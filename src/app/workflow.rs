use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread;

use atomic_write_file::AtomicWriteFile;
use base64::Engine;

use crate::agc::auth::{AuthClient, TokenData};
use crate::agc::client::AgcClient;
use crate::device::hdc::{DeviceInfo, HdcClient};
use crate::hap::info::read_hap_info;
use crate::hap::sign::sign_hap;
use crate::materials::generate::generate_key_material;
use crate::materials::manager::{
    ManagerError, PrepareRequest, find_local_materials, prepare_materials_with_api,
};
use crate::materials::store::{MaterialStore, SystemSecretStore};

use super::state::{AppPhase, LogLevel, WorkflowEvent};

const TOKEN_SECRET: &str = "agc-token";

pub fn spawn_resign(input: PathBuf, device: DeviceInfo, events: Sender<WorkflowEvent>) {
    thread::spawn(move || {
        if let Err(error) = resign(&input, &device, &events) {
            let _ = events.send(WorkflowEvent::Failed(error));
        }
    });
}

pub fn spawn_device_scan(events: Sender<WorkflowEvent>) {
    thread::spawn(move || {
        let result = HdcClient::discover().and_then(|hdc| hdc.list_devices());
        let event = match result {
            Ok(devices) => WorkflowEvent::Devices(devices),
            Err(error) => WorkflowEvent::DeviceScanFailed(error.to_string()),
        };
        let _ = events.send(event);
    });
}

pub fn spawn_install(
    signed_hap: PathBuf,
    bundle_name: String,
    ability: String,
    device: DeviceInfo,
    events: Sender<WorkflowEvent>,
) {
    thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            emit_phase(&events, AppPhase::Installing);
            let hdc = HdcClient::discover().map_err(|error| error.to_string())?;
            hdc.install(&device, &signed_hap)
                .map_err(|error| error.to_string())?;
            emit(&events, LogLevel::Success, "HAP 安装成功");
            hdc.launch(&device, &bundle_name, &ability)
                .map_err(|error| error.to_string())?;
            let pid = hdc
                .pid(&device, &bundle_name)
                .map_err(|error| error.to_string())?;
            events
                .send(WorkflowEvent::Installed { pid })
                .map_err(|error| error.to_string())?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = events.send(WorkflowEvent::Failed(error));
        }
    });
}

fn resign(input: &Path, device: &DeviceInfo, events: &Sender<WorkflowEvent>) -> Result<(), String> {
    emit_phase(events, AppPhase::Inspecting);
    let info = read_hap_info(input).map_err(|error| error.to_string())?;
    emit(
        events,
        LogLevel::Success,
        &format!(
            "识别 HAP：{} {}",
            info.bundle_name,
            info.version_name.as_deref().unwrap_or("")
        ),
    );

    emit(
        events,
        LogLevel::Success,
        &format!("设备：{}", device.model),
    );

    let store = material_store()?;
    let secrets = SystemSecretStore;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let password_candidates = |path: &Path| passwords_for(&secrets, path);
    let prepared = if let Some(materials) = find_local_materials(
        &store,
        &info.bundle_name,
        &device.udid,
        now,
        password_candidates,
    )
    .map_err(|error| error.to_string())?
    {
        emit(events, LogLevel::Success, "复用本地证书与 Profile");
        materials
    } else {
        emit_phase(events, AppPhase::Authenticating);
        let auth = AuthClient::new().map_err(|error| error.to_string())?;
        let token = load_or_login(&auth, &secrets, events)?;
        let api = AgcClient::new(token.clone()).map_err(|error| error.to_string())?;
        emit_phase(events, AppPhase::PreparingMaterials);
        let timestamp = time::OffsetDateTime::now_utc().unix_timestamp();
        let password = random_password();
        let certificate_nonce = random_label_suffix();
        let request = PrepareRequest {
            bundle_name: &info.bundle_name,
            udid: &device.udid,
            now_unix: now,
            team_id: &token.team_id,
            new_password: &password,
            device_name: &format!("hap_resigner_{timestamp}"),
            device_type: "4",
            certificate_name: &format!("hap_resigner_debug_{timestamp}_{certificate_nonce}"),
            provision_name: &format!("hap_resigner_profile_{timestamp}"),
        };
        prepare_materials_with_api(
            &store,
            &api,
            &request,
            |path| passwords_for(&secrets, path),
            |path| pending_passwords_for(&secrets, path),
            |path, password| save_password(&secrets, path, password),
            |team, password| {
                generate_key_material(team, password)
                    .map_err(|error| ManagerError::InvalidMaterial(error.to_string()))
            },
        )
        .map_err(|error| error.to_string())?
    };

    emit_phase(events, AppPhase::Signing);
    let input_bytes = fs::read(input).map_err(|error| error.to_string())?;
    let output_bytes = sign_hap(&input_bytes, &prepared.identity, &prepared.profile)
        .map_err(|error| error.to_string())?;
    let output = output_path(input);
    atomic_write(&output, &output_bytes).map_err(|error| error.to_string())?;
    emit(
        events,
        LogLevel::Success,
        &format!("签名完成：{}", output.display()),
    );
    events
        .send(WorkflowEvent::Signed {
            output,
            bundle_name: info.bundle_name,
            ability: info
                .main_element
                .unwrap_or_else(|| "EntryAbility".to_owned()),
        })
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn load_or_login(
    auth: &AuthClient,
    secrets: &SystemSecretStore,
    events: &Sender<WorkflowEvent>,
) -> Result<TokenData, String> {
    if let Ok(bytes) = secrets.get(TOKEN_SECRET)
        && let Ok(mut token) = serde_json::from_slice::<TokenData>(&bytes)
        && auth.refresh_token(&mut token).is_ok()
    {
        save_token(secrets, &token)?;
        emit(events, LogLevel::Success, "华为账号令牌已刷新");
        return Ok(token);
    }
    emit(events, LogLevel::Info, "正在打开浏览器登录华为账号");
    emit_phase(events, AppPhase::WaitingForLogin);
    let token = auth
        .browser_login(10_101)
        .map_err(|error| error.to_string())?;
    save_token(secrets, &token)?;
    Ok(token)
}

fn save_token(secrets: &SystemSecretStore, token: &TokenData) -> Result<(), String> {
    let bytes = serde_json::to_vec(token).map_err(|error| error.to_string())?;
    secrets
        .set(TOKEN_SECRET, &bytes)
        .map_err(|error| error.to_string())
}

fn material_store() -> Result<MaterialStore, String> {
    if let Some(home) = std::env::var_os("HOME") {
        let legacy = PathBuf::from(home).join(".hap-resigner");
        if legacy.join("materials").is_dir() {
            return Ok(MaterialStore::at(legacy));
        }
    }
    MaterialStore::system().map_err(|error| error.to_string())
}

fn passwords_for(secrets: &SystemSecretStore, path: &Path) -> Vec<String> {
    let name = password_secret_name(path);
    let mut passwords = secrets
        .get(&name)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .into_iter()
        .collect::<Vec<_>>();
    if !passwords.iter().any(|password| password == "123456") {
        passwords.push("123456".to_owned());
    }
    passwords
}

fn pending_passwords_for(
    secrets: &SystemSecretStore,
    path: &Path,
) -> Result<Vec<String>, ManagerError> {
    let bytes = secrets
        .get_optional(&password_secret_name(path))
        .map_err(|error| ManagerError::InvalidMaterial(error.to_string()))?
        .ok_or_else(|| {
            ManagerError::InvalidMaterial(
                "pending P12 password is missing from the system credential store".to_owned(),
            )
        })?;
    let password = String::from_utf8(bytes).map_err(|_| {
        ManagerError::InvalidMaterial(
            "pending P12 password in the system credential store is not UTF-8".to_owned(),
        )
    })?;
    Ok(vec![password])
}

fn save_password(
    secrets: &SystemSecretStore,
    path: &Path,
    password: &str,
) -> Result<(), ManagerError> {
    secrets
        .set(&password_secret_name(path), password.as_bytes())
        .map_err(|error| ManagerError::InvalidMaterial(error.to_string()))
}

fn password_secret_name(path: &Path) -> String {
    format!(
        "p12:{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("default")
    )
}

fn random_password() -> String {
    let bytes = (0..24).map(|_| rand::random::<u8>()).collect::<Vec<_>>();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn random_label_suffix() -> String {
    let bytes = (0..9).map(|_| rand::random::<u8>()).collect::<Vec<_>>();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn output_path(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("app");
    input.with_file_name(format!("{stem}-resigned.hap"))
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), std::io::Error> {
    let mut file = AtomicWriteFile::options().open(path)?;
    file.write_all(contents)?;
    file.commit()
}

fn emit_phase(events: &Sender<WorkflowEvent>, phase: AppPhase) {
    let _ = events.send(WorkflowEvent::Phase(phase));
}

fn emit(events: &Sender<WorkflowEvent>, level: LogLevel, message: &str) {
    let _ = events.send(WorkflowEvent::Progress {
        level,
        message: message.to_owned(),
    });
}

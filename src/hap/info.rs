use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HapInfoError {
    #[error("failed to open HAP: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid HAP ZIP: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("invalid module.json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("module.json has no bundleName")]
    MissingBundleName,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HapInfo {
    pub bundle_name: String,
    pub version_name: Option<String>,
    pub main_element: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModuleJson {
    #[serde(default)]
    app: AppInfo,
    #[serde(default)]
    module: ModuleInfo,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    #[serde(default)]
    bundle_name: String,
    version_name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModuleInfo {
    main_element: Option<String>,
}

pub fn read_hap_info(path: &Path) -> Result<HapInfo, HapInfoError> {
    let file = File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut module_file = archive.by_name("module.json")?;
    let mut json = String::new();
    module_file.read_to_string(&mut json)?;
    let module: ModuleJson = serde_json::from_str(&json)?;
    if module.app.bundle_name.is_empty() {
        return Err(HapInfoError::MissingBundleName);
    }
    Ok(HapInfo {
        bundle_name: module.app.bundle_name,
        version_name: module.app.version_name,
        main_element: module.module.main_element,
    })
}

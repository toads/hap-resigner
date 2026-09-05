use std::thread;
use std::time::Duration;

use reqwest::Method;
use reqwest::blocking::{Client, RequestBuilder};
use reqwest::header::CONTENT_TYPE;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use super::auth::TokenData;
use super::http::{http_client_builder, redact_embedded_urls, safe_transport_error};

const AGC_BASE: &str = "https://connect-api.cloud.huawei.com";
const DEVICE_LIST: &str = "/api/cps/device-manage/v1/device/list";
const DEVICE_ADD: &str = "/api/cps/device-manage/v1/device/add";
const CERT_LIST: &str = "/api/cps/harmony-cert-manage/v1/cert/list";
const CERT_ADD: &str = "/api/cps/harmony-cert-manage/v1/cert/add";
const PROVISION_ADD_TEST: &str = "/api/cps/provision-manage/v1/ide/test/provision/add";
const DOWNLOAD_URL: &str = "/api/amis/app-manage/v1/objects/url/reapply";
const USER_AGENT: &str = "Chrome/49.0.2623.75";
pub const CERTIFICATE_MANAGEMENT_URL: &str =
    "https://developer.huawei.com/consumer/cn/service/josp/agc/index.html";
const CERTIFICATE_LOOKUP_ATTEMPTS: usize = 5;
const CERTIFICATE_LOOKUP_DELAY: Duration = Duration::from_millis(250);
const DEVICE_LOOKUP_ATTEMPTS: usize = CERTIFICATE_LOOKUP_ATTEMPTS;
const DEVICE_LOOKUP_DELAY: Duration = CERTIFICATE_LOOKUP_DELAY;

#[derive(Debug, Error)]
pub enum AgcError {
    #[error("AGC HTTP request failed: {0}")]
    Http(String),
    #[error("AGC {endpoint} returned HTTP {status} ({content_type}): {body}")]
    Response {
        endpoint: &'static str,
        status: u16,
        content_type: String,
        body: String,
    },
    #[error(
        "AGC {endpoint} returned an invalid response (HTTP {status}, {content_type}); body: {body}"
    )]
    InvalidResponse {
        endpoint: &'static str,
        status: u16,
        content_type: String,
        body: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("AGC {endpoint} returned an unexpected JSON shape; body: {body}")]
    InvalidPayload {
        endpoint: &'static str,
        body: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("AGC rejected the request: {0}")]
    Rejected(String),
    #[error(
        "AGC accepted device registration, but no complete device id appeared after {attempts} list attempts; response: {response}; last lookup error: {last_lookup_error}"
    )]
    DeviceNotVisible {
        attempts: usize,
        response: String,
        last_lookup_error: String,
    },
    #[error(
        "AGC returned {matches} complete device records for the requested UDID; refusing an ambiguous device association"
    )]
    DeviceUdidAmbiguous { matches: usize },
    #[error(
        "CERTIFICATE_LIMIT_REACHED: {message}。请前往 AGC 管理并废除历史证书：{management_url}"
    )]
    CertificateLimit {
        message: String,
        management_url: &'static str,
    },
    #[error(
        "AGC accepted certificate {certificate_name}, but no complete id/URL appeared after {attempts} list attempts; create response: {response}; last lookup error: {last_lookup_error}"
    )]
    CertificateNotVisible {
        certificate_name: String,
        attempts: usize,
        response: String,
        last_lookup_error: String,
    },
    #[error(
        "AGC returned {matches} complete certificates named {certificate_name}; refusing an ambiguous private-key association"
    )]
    CertificateNameAmbiguous {
        certificate_name: String,
        matches: usize,
    },
    #[error("AGC response is missing {0}")]
    Missing(&'static str),
}

impl From<reqwest::Error> for AgcError {
    fn from(error: reqwest::Error) -> Self {
        Self::Http(safe_transport_error(&error))
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRecord {
    #[serde(alias = "deviceId")]
    pub id: Option<String>,
    #[serde(default)]
    pub udid: String,
    #[serde(default)]
    pub device_name: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CertificateRecord {
    #[serde(alias = "certId")]
    pub id: Option<String>,
    #[serde(default)]
    pub cert_name: String,
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub cert_type: String,
    #[serde(
        default,
        alias = "certObjectId",
        alias = "certFileUrl",
        alias = "fileUrl"
    )]
    pub cert_url: String,
}

fn deserialize_string_or_number<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    match Value::deserialize(deserializer)? {
        Value::String(value) => Ok(value),
        Value::Number(value) => Ok(value.to_string()),
        Value::Null => Ok(String::new()),
        _ => Err(serde::de::Error::custom("expected a string or number")),
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProvisionRecord {
    pub id: Option<String>,
    pub url: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct AgcClient {
    http: Client,
    base: String,
    token: TokenData,
}

impl AgcClient {
    pub fn new(token: TokenData) -> Result<Self, AgcError> {
        Self::with_base(AGC_BASE, token)
    }

    pub fn with_base(base: &str, token: TokenData) -> Result<Self, AgcError> {
        let http = http_client_builder(Duration::from_secs(60), USER_AGENT).build()?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_owned(),
            token,
        })
    }

    pub fn list_devices(&self) -> Result<Vec<DeviceRecord>, AgcError> {
        let value = self.send_json_value(
            self.request(Method::GET, DEVICE_LIST).query(&[
                ("encodeFlag", "0"),
                ("start", "1"),
                ("pageSize", "100"),
            ]),
            DEVICE_LIST,
            true,
        )?;
        self.check_response(DEVICE_LIST, &value)?;
        Ok(self
            .decode_value::<DeviceListResponse>(DEVICE_LIST, value)?
            .list)
    }

    pub fn register_device(
        &self,
        udid: &str,
        device_type: &str,
        device_name: &str,
    ) -> Result<DeviceRecord, AgcError> {
        let initial_devices = self.list_devices()?;
        if let Some(device) = unique_complete_device(&initial_devices, udid)? {
            return Ok(device);
        }
        let existing_record_seen = initial_devices
            .iter()
            .any(|device| device.udid.eq_ignore_ascii_case(udid));
        let mut response_summary = "existing device record remained incomplete".to_owned();

        if !existing_record_seen {
            let response = self.send_json_value(
                self.request(Method::POST, DEVICE_ADD).json(&json!({
                    "deviceName": device_name,
                    "udid": udid,
                    "deviceType": device_type,
                })),
                DEVICE_ADD,
                false,
            )?;
            response_summary = "device add response did not contain a complete id".to_owned();
            let duplicate = response_message(&response).is_some_and(is_duplicate_device_message);
            match self.check_response(DEVICE_ADD, &response) {
                Ok(()) => {}
                Err(AgcError::Rejected(_)) if duplicate => {}
                Err(error) => return Err(error),
            }
            if !duplicate {
                let mut direct: DeviceRecord = self.decode_value(DEVICE_ADD, response)?;
                if device_record_is_complete(&direct)
                    && (direct.udid.is_empty() || direct.udid.eq_ignore_ascii_case(udid))
                {
                    if direct.udid.is_empty() {
                        direct.udid = udid.to_owned();
                    }
                    if direct.device_name.is_empty() {
                        direct.device_name = device_name.to_owned();
                    }
                    return Ok(direct);
                }
            }
        }

        let mut last_lookup_error = "none".to_owned();
        for attempt in 0..DEVICE_LOOKUP_ATTEMPTS {
            if attempt > 0 {
                thread::sleep(DEVICE_LOOKUP_DELAY);
            }
            match self.list_devices() {
                Ok(devices) => {
                    if let Some(device) = unique_complete_device(&devices, udid)? {
                        return Ok(device);
                    }
                }
                Err(error) => {
                    last_lookup_error = error.to_string().replace(udid, "<redacted>");
                }
            }
        }
        Err(AgcError::DeviceNotVisible {
            attempts: DEVICE_LOOKUP_ATTEMPTS,
            response: response_summary,
            last_lookup_error,
        })
    }

    pub fn list_certificates(&self) -> Result<Vec<CertificateRecord>, AgcError> {
        let value = self.send_json_value(
            self.request(Method::POST, CERT_LIST).json(&json!({})),
            CERT_LIST,
            true,
        )?;
        self.check_response(CERT_LIST, &value)?;
        let response: CertificateListResponse = self.decode_value(CERT_LIST, value)?;
        Ok(if response.cert_list.is_empty() {
            response.list
        } else {
            response.cert_list
        })
    }

    pub fn upload_csr(
        &self,
        csr_pem: &str,
        certificate_name: &str,
    ) -> Result<CertificateRecord, AgcError> {
        let response = self.send_json_value(
            self.request(Method::POST, CERT_ADD).json(&json!({
                "csr": csr_pem.trim(),
                "certName": certificate_name,
                "certType": "1",
            })),
            CERT_ADD,
            false,
        )?;
        let response_preview = self.response_value_preview(&response);
        let duplicate_name = response_message(&response).is_some_and(is_duplicate_name_message);
        match self.check_response(CERT_ADD, &response) {
            Ok(()) => {}
            Err(AgcError::Rejected(_)) if duplicate_name => {}
            Err(error) => return Err(error),
        }
        let direct: CertificateRecord = if duplicate_name {
            CertificateRecord::default()
        } else {
            self.decode_value(CERT_ADD, response)?
        };
        if certificate_record_is_complete(&direct) {
            return Ok(direct);
        }

        let mut last_lookup_error = "none".to_owned();
        for attempt in 0..CERTIFICATE_LOOKUP_ATTEMPTS {
            if attempt > 0 {
                thread::sleep(CERTIFICATE_LOOKUP_DELAY);
            }
            match self.list_certificates() {
                Ok(certificates) => {
                    let matches = certificates
                        .into_iter()
                        .filter(|certificate| {
                            certificate.cert_name == certificate_name
                                && certificate_record_is_complete(certificate)
                        })
                        .collect::<Vec<_>>();
                    match matches.len() {
                        0 => {}
                        1 => return Ok(matches.into_iter().next().expect("one match")),
                        count => {
                            return Err(AgcError::CertificateNameAmbiguous {
                                certificate_name: certificate_name.to_owned(),
                                matches: count,
                            });
                        }
                    }
                }
                Err(error) => last_lookup_error = error.to_string(),
            }
        }
        Err(AgcError::CertificateNotVisible {
            certificate_name: certificate_name.to_owned(),
            attempts: CERTIFICATE_LOOKUP_ATTEMPTS,
            response: response_preview,
            last_lookup_error,
        })
    }

    pub fn create_test_provision(
        &self,
        certificate_ids: &[String],
        device_ids: &[String],
        bundle_name: &str,
        provision_name: &str,
    ) -> Result<ProvisionRecord, AgcError> {
        let response = self.send_json_value(
            self.request(Method::POST, PROVISION_ADD_TEST).json(&json!({
                "certList": certificate_ids,
                "packageName": bundle_name,
                "deviceList": device_ids,
                "provisionName": provision_name,
                "aclPermissionList": [],
            })),
            PROVISION_ADD_TEST,
            false,
        )?;
        self.check_response(PROVISION_ADD_TEST, &response)?;
        let url = response
            .get("provisionFileUrl")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or(AgcError::Missing("provisionFileUrl"))?
            .to_owned();
        Ok(ProvisionRecord {
            id: response
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            url,
            name: provision_name.to_owned(),
        })
    }

    pub fn download_object(&self, source_url: &str) -> Result<Vec<u8>, AgcError> {
        let value = self.send_json_value(
            self.request(Method::POST, DOWNLOAD_URL)
                .json(&json!({"sourceUrls": source_url})),
            DOWNLOAD_URL,
            false,
        )?;
        self.check_response(DOWNLOAD_URL, &value)?;
        let response: UrlResponse = self.decode_value(DOWNLOAD_URL, value)?;
        let url = response
            .urls_info
            .into_iter()
            .next()
            .map(|item| item.new_url)
            .filter(|value| !value.is_empty())
            .ok_or(AgcError::Missing("urlsInfo[0].newUrl"))?;
        Ok(self
            .http
            .get(url)
            .send()?
            .error_for_status()?
            .bytes()?
            .to_vec())
    }

    fn send_json_value(
        &self,
        request: RequestBuilder,
        endpoint: &'static str,
        allow_empty: bool,
    ) -> Result<Value, AgcError> {
        let response = request.send()?;
        let status = response.status();
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("<missing>")
            .to_owned();
        let body = response.bytes()?;
        let body = body.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&body);
        let preview = self.response_preview(body);
        if !status.is_success() {
            return Err(AgcError::Response {
                endpoint,
                status: status.as_u16(),
                content_type,
                body: preview,
            });
        }
        if allow_empty && body.iter().all(u8::is_ascii_whitespace) {
            return Ok(json!({}));
        }
        serde_json::from_slice(body).map_err(|source| AgcError::InvalidResponse {
            endpoint,
            status: status.as_u16(),
            content_type,
            body: preview,
            source,
        })
    }

    fn decode_value<T: DeserializeOwned>(
        &self,
        endpoint: &'static str,
        value: Value,
    ) -> Result<T, AgcError> {
        let preview = self.response_value_preview(&value);
        serde_json::from_value(value).map_err(|source| AgcError::InvalidPayload {
            endpoint,
            body: preview,
            source,
        })
    }

    fn response_preview(&self, body: &[u8]) -> String {
        if let Ok(mut value) = serde_json::from_slice::<Value>(body) {
            redact_sensitive_json(&mut value);
            return self.text_preview(&value.to_string());
        }
        if body.iter().all(u8::is_ascii_whitespace) {
            "<empty>".to_owned()
        } else {
            "non-JSON response body: <redacted>".to_owned()
        }
    }
    fn response_value_preview(&self, value: &Value) -> String {
        let mut value = value.clone();
        redact_sensitive_json(&mut value);
        self.text_preview(&value.to_string())
    }

    fn text_preview(&self, text: &str) -> String {
        let mut text = text.trim().replace('\r', " ").replace('\n', " ");
        for secret in [
            &self.token.access_token,
            &self.token.refresh_token,
            &self.token.jwt_token,
        ] {
            if !secret.is_empty() {
                text = text.replace(secret, "<redacted>");
            }
        }
        text = redact_pem_blocks(&text);
        text = redact_embedded_urls(&text);
        let truncated = text.chars().count() > 512;
        let mut preview = text.chars().take(512).collect::<String>();
        if truncated {
            preview.push('…');
        }
        if preview.is_empty() {
            preview.push_str("<empty>");
        }
        preview
    }

    fn check_response(&self, endpoint: &'static str, response: &Value) -> Result<(), AgcError> {
        let ret = response.get("ret").unwrap_or(response);
        let Some(code) = ret.get("code") else {
            return Ok(());
        };
        let code_ok = code.as_i64() == Some(0) || code.as_str() == Some("0");
        if code_ok {
            return Ok(());
        }
        let raw_message = response_message(response).unwrap_or("unknown AGC error");
        if endpoint == CERT_ADD && is_certificate_limit_message(raw_message) {
            Err(AgcError::CertificateLimit {
                message: "certificate quota reached".to_owned(),
                management_url: CERTIFICATE_MANAGEMENT_URL,
            })
        } else {
            Err(AgcError::Rejected(format!(
                "{endpoint}; code={}; remote message: <redacted>",
                safe_error_code(code)
            )))
        }
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        self.http
            .request(method, format!("{}{}", self.base, path))
            .header("uid", &self.token.user_id)
            .header("teamId", &self.token.team_id)
            .header("oauth2Token", &self.token.access_token)
            .header("accept-language", "zh-CN")
    }
}

#[derive(Debug, Deserialize)]
struct DeviceListResponse {
    #[serde(default)]
    list: Vec<DeviceRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CertificateListResponse {
    #[serde(default)]
    cert_list: Vec<CertificateRecord>,
    #[serde(default)]
    list: Vec<CertificateRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UrlResponse {
    #[serde(default)]
    urls_info: Vec<UrlInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UrlInfo {
    #[serde(default)]
    new_url: String,
}

fn certificate_record_is_complete(certificate: &CertificateRecord) -> bool {
    certificate.id.as_deref().is_some_and(|id| !id.is_empty()) && !certificate.cert_url.is_empty()
}

fn device_record_is_complete(device: &DeviceRecord) -> bool {
    device.id.as_deref().is_some_and(|id| !id.is_empty())
}

fn unique_complete_device(
    devices: &[DeviceRecord],
    expected_udid: &str,
) -> Result<Option<DeviceRecord>, AgcError> {
    let mut selected = None;
    let mut matches = 0;
    for device in devices {
        if device.udid.eq_ignore_ascii_case(expected_udid) && device_record_is_complete(device) {
            matches += 1;
            if selected.is_none() {
                selected = Some(device.clone());
            }
        }
    }
    if matches > 1 {
        Err(AgcError::DeviceUdidAmbiguous { matches })
    } else {
        Ok(selected)
    }
}

fn is_duplicate_device_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("already exist")
        || lower.contains("duplicate")
        || lower.contains("repeat")
        || message.contains("已存在")
        || message.contains("重复")
}
fn is_certificate_limit_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    if lower.contains("name")
        || lower.contains("length")
        || message.contains("名称")
        || message.contains("长度")
    {
        return false;
    }
    let english_quantity =
        contains_any_ascii_word(&lower, &["number", "quantity", "count", "quota"]);
    let english_limit = contains_any_ascii_word(
        &lower,
        &[
            "limit", "maximum", "upper", "exceed", "exceeds", "exceeded", "reach", "reaches",
            "reached", "full",
        ],
    );
    let chinese_quantity = ["数量", "个数", "配额"]
        .iter()
        .any(|word| message.contains(word));
    let chinese_limit = ["上限", "最大", "已满", "限制"]
        .iter()
        .any(|word| message.contains(word));
    (english_quantity && english_limit) || (chinese_quantity && chinese_limit)
}

fn contains_any_ascii_word(text: &str, expected: &[&str]) -> bool {
    text.split(|character: char| !character.is_ascii_alphanumeric())
        .any(|word| expected.contains(&word))
}

fn is_duplicate_name_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    (lower.contains("certificate")
        && (lower.contains("already exist")
            || lower.contains("duplicate")
            || lower.contains("same name")))
        || (message.contains("证书") && (message.contains("已存在") || message.contains("重名")))
}

fn safe_error_code(code: &Value) -> String {
    match code {
        Value::Number(number) => number.to_string(),
        Value::String(value)
            if value.len() <= 32
                && !value.is_empty()
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'-') =>
        {
            value.clone()
        }
        _ => "<redacted>".to_owned(),
    }
}

fn response_message(response: &Value) -> Option<&str> {
    response
        .get("ret")
        .unwrap_or(response)
        .get("msg")
        .and_then(Value::as_str)
}

fn redact_sensitive_json(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                let key = key.to_ascii_lowercase().replace(['_', '-'], "");
                if key.contains("csr")
                    || key.contains("token")
                    || key.contains("authorization")
                    || key.contains("password")
                    || key.contains("secret")
                    || key.contains("url")
                    || key.contains("objectid")
                    || key.contains("sourceuri")
                    || key == "udid"
                    || key == "deviceid"
                {
                    *value = Value::String("<redacted>".to_owned());
                } else {
                    redact_sensitive_json(value);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_sensitive_json(item);
            }
        }
        _ => {}
    }
}

fn redact_pem_blocks(text: &str) -> String {
    let mut redacted = text.to_owned();
    for label in [
        "NEW CERTIFICATE REQUEST",
        "CERTIFICATE REQUEST",
        "PRIVATE KEY",
        "CERTIFICATE",
    ] {
        let begin = format!("-----BEGIN {label}-----");
        let end = format!("-----END {label}-----");
        while let Some(start) = redacted.find(&begin) {
            let suffix = &redacted[start + begin.len()..];
            let replace_end = if let Some(relative_end) = suffix.find(&end) {
                start + begin.len() + relative_end + end.len()
            } else {
                redacted.len()
            };
            redacted.replace_range(start..replace_end, "<redacted-pem>");
        }
    }
    redacted
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{AgcClient, AgcError, DEVICE_ADD, TokenData, is_certificate_limit_message};

    #[test]
    fn recognizes_english_and_chinese_certificate_limits() {
        assert!(is_certificate_limit_message(
            "The number of certificates exceeds limit"
        ));
        assert!(is_certificate_limit_message("Certificate quota exceeded"));
        assert!(is_certificate_limit_message("Certificate quota exceeds"));
        assert!(is_certificate_limit_message(
            "Certificate quota reaches maximum"
        ));
        assert!(is_certificate_limit_message("调试证书数量已达到上限"));
        assert!(is_certificate_limit_message(
            "The quantity has reached the upper limit"
        ));
        assert!(is_certificate_limit_message(
            "Certificate count has reached the maximum"
        ));
        assert!(is_certificate_limit_message("数量已达到最大值"));
        assert!(!is_certificate_limit_message("certificate created"));
        assert!(!is_certificate_limit_message(
            "Certificate name exceeds the maximum length"
        ));
        assert!(!is_certificate_limit_message(
            "Account request limit has been reached"
        ));
    }

    #[test]
    fn certificate_quota_classification_is_limited_to_cert_add() {
        let client = AgcClient::with_base(
            "http://127.0.0.1",
            TokenData {
                access_token: "access".to_owned(),
                refresh_token: String::new(),
                user_id: "user".to_owned(),
                jwt_token: String::new(),
                team_id: "team".to_owned(),
                teams: BTreeMap::new(),
            },
        )
        .unwrap();
        let error = client
            .check_response(
                DEVICE_ADD,
                &serde_json::json!({
                    "ret": {"code": 123, "msg": "The number of certificates exceeds limit"}
                }),
            )
            .expect_err("non-certificate endpoint must remain a generic rejection");
        assert!(matches!(error, AgcError::Rejected(_)));
    }

    #[test]
    fn response_preview_redacts_a_long_token_before_truncation() {
        let token = "sensitive-token-".repeat(50);
        let client = AgcClient::with_base(
            "http://127.0.0.1",
            TokenData {
                access_token: token.clone(),
                refresh_token: String::new(),
                user_id: "user".to_owned(),
                jwt_token: String::new(),
                team_id: "team".to_owned(),
                teams: BTreeMap::new(),
            },
        )
        .unwrap();
        let preview = client.response_preview(format!("prefix:{token}:suffix").as_bytes());
        assert!(preview.contains("<redacted>"), "{preview}");
        assert!(!preview.contains("sensitive-token"), "{preview}");
    }

    #[test]
    fn raw_json_preview_redacts_sensitive_fields_before_decode() {
        let client = AgcClient::with_base(
            "http://127.0.0.1",
            TokenData {
                access_token: "access".to_owned(),
                refresh_token: String::new(),
                user_id: "user".to_owned(),
                jwt_token: String::new(),
                team_id: "team".to_owned(),
                teams: BTreeMap::new(),
            },
        )
        .unwrap();
        let preview = client.response_preview(
            br#"{"csr":"raw-csr-secret","certObjectId":"CN/private-object","newUrl":"https://obs.example/private?Signature=secret","oauth2Token":"unknown-token"}"#,
        );
        for secret in [
            "raw-csr-secret",
            "CN/private-object",
            "Signature",
            "unknown-token",
        ] {
            assert!(
                !preview.contains(secret),
                "sensitive raw JSON leaked: {preview}"
            );
        }
    }
}

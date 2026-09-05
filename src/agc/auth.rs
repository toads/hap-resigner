use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use super::http::{http_client_builder, redact_embedded_urls, safe_transport_error};
use base64::Engine;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

const AGC_BASE: &str = "https://connect-api.cloud.huawei.com";
const AUTH_BASE: &str = "https://cn.devecostudio.huawei.com";
const LOGIN_PAGE: &str = "/console/DevEcoIDE/apply";
const TEMP_TOKEN_CHECK: &str = "/authrouter/auth/api/temptoken/check";
const JWT_CHECK: &str = "/authrouter/auth/api/jwToken/check";
const USER_TEAM: &str = "/api/ups/user-permission-service/v1/user-team-list";
const APP_ID: &str = "1007";
const CLIENT_VERSION: &str = "6.0.2";
const USER_AGENT: &str = "Chrome/49.0.2623.75";

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("HTTP request failed: {0}")]
    Http(String),
    #[error("local callback failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("browser login timed out")]
    Timeout,
    #[error("invalid temporary token")]
    InvalidTempToken,
    #[error("invalid JWT returned by authentication service")]
    InvalidJwt,
    #[error("authentication service rejected the token: {0}")]
    Rejected(String),
    #[error("failed to open system browser: {0}")]
    Browser(String),
}

impl From<reqwest::Error> for AuthError {
    fn from(error: reqwest::Error) -> Self {
        Self::Http(safe_transport_error(&error))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTempToken {
    pub actual_token: String,
    pub site_code: &'static str,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenData {
    pub access_token: String,
    pub refresh_token: String,
    pub user_id: String,
    pub jwt_token: String,
    pub team_id: String,
    #[serde(default)]
    pub teams: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct AuthClient {
    http: Client,
    auth_base: String,
    agc_base: String,
}

impl AuthClient {
    pub fn new() -> Result<Self, AuthError> {
        Self::with_bases(AUTH_BASE, AGC_BASE)
    }

    pub fn with_bases(auth_base: &str, agc_base: &str) -> Result<Self, AuthError> {
        let http = http_client_builder(Duration::from_secs(30), USER_AGENT).build()?;
        Ok(Self {
            http,
            auth_base: auth_base.trim_end_matches('/').to_owned(),
            agc_base: agc_base.trim_end_matches('/').to_owned(),
        })
    }

    pub fn refresh_token(&self, token: &mut TokenData) -> Result<(), AuthError> {
        if token.jwt_token.is_empty() {
            return Err(AuthError::InvalidJwt);
        }
        let response = self.jwt_check(&token.jwt_token, true)?;
        let user = response
            .user_info
            .ok_or_else(|| AuthError::Rejected("userInfo is missing".to_owned()))?;
        token.access_token = user.access_token;
        if !user.refresh_token.is_empty() {
            token.refresh_token = user.refresh_token;
        }
        if token.user_id.is_empty() {
            token.user_id = user.user_id;
        }
        Ok(())
    }

    pub fn browser_login(&self, preferred_port: u16) -> Result<TokenData, AuthError> {
        let receiver = CallbackReceiver::bind(preferred_port)?;
        let mut login_url = Url::parse(&format!("{}{}", self.auth_base, LOGIN_PAGE))
            .map_err(|_| AuthError::InvalidTempToken)?;
        login_url
            .query_pairs_mut()
            .append_pair("port", &receiver.port().to_string())
            .append_pair("appid", APP_ID)
            .append_pair("code", receiver.client_secret());
        open::that(login_url.as_str()).map_err(|error| browser_launch_error(&error.to_string()))?;
        let temp_token = receiver.wait_for_token(Duration::from_secs(300))?;
        self.exchange_temp_token(&temp_token)
    }

    pub fn exchange_temp_token(&self, raw_token: &str) -> Result<TokenData, AuthError> {
        let parsed = parse_temp_token(raw_token)?;
        let jwt_token = self
            .http
            .get(format!("{}{}", self.auth_base, TEMP_TOKEN_CHECK))
            .header("accept-language", "zh-CN")
            .header("Accept-Encoding", "identity")
            .query(&[
                ("tempToken", parsed.actual_token.as_str()),
                ("site", parsed.site_code),
                ("version", CLIENT_VERSION),
                ("appid", APP_ID),
            ])
            .send()?
            .error_for_status()?
            .text()?
            .trim()
            .to_owned();
        if jwt_token.split('.').count() != 3 {
            return Err(AuthError::InvalidJwt);
        }
        let response = self.jwt_check(&jwt_token, false)?;
        let user = response
            .user_info
            .ok_or_else(|| AuthError::Rejected("userInfo is missing".to_owned()))?;
        let user_id = if user.user_id.is_empty() {
            jwt_user_id(&jwt_token)?
        } else {
            user.user_id
        };
        let (teams, team_id) = self.fetch_teams(&user_id, &user.access_token);
        Ok(TokenData {
            access_token: user.access_token,
            refresh_token: user.refresh_token,
            user_id,
            jwt_token,
            team_id,
            teams,
        })
    }

    fn jwt_check(&self, jwt_token: &str, refresh: bool) -> Result<JwtResponse, AuthError> {
        let response: JwtResponse = self
            .http
            .get(format!("{}{}", self.auth_base, JWT_CHECK))
            .header("jwtToken", jwt_token)
            .header("refresh", if refresh { "true" } else { "false" })
            .header("accept-language", "zh-CN")
            .send()?
            .error_for_status()?
            .json()?;
        if !response.status {
            return Err(AuthError::Rejected("status=false".to_owned()));
        }
        Ok(response)
    }

    fn fetch_teams(&self, user_id: &str, access_token: &str) -> (BTreeMap<String, String>, String) {
        let response = self
            .http
            .get(format!("{}{}", self.agc_base, USER_TEAM))
            .header("uid", user_id)
            .header("oauth2Token", access_token)
            .header("accept-language", "zh-CN")
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(reqwest::blocking::Response::json::<TeamResponse>);
        let Ok(response) = response else {
            return (BTreeMap::new(), user_id.to_owned());
        };
        let mut teams = BTreeMap::new();
        let mut first_team = None;
        for team in response.teams {
            if team.id.is_empty() || team.name.is_empty() {
                continue;
            }
            first_team.get_or_insert_with(|| team.id.clone());
            teams.insert(team.id, team.name);
        }
        let team_id = first_team.unwrap_or_else(|| user_id.to_owned());
        (teams, team_id)
    }
}

pub fn parse_temp_token(raw_token: &str) -> Result<ParsedTempToken, AuthError> {
    let raw_token = raw_token.trim();
    if raw_token.is_empty() {
        return Err(AuthError::InvalidTempToken);
    }
    let site_code = match raw_token.as_bytes().last().copied() {
        Some(b'1') => "CN",
        Some(b'5') => "SG",
        Some(b'7') => "DE",
        Some(b'8') => "RU",
        _ => "CN",
    };
    let actual_token = raw_token
        .split('&')
        .next()
        .filter(|token| !token.is_empty())
        .ok_or(AuthError::InvalidTempToken)?
        .to_owned();
    Ok(ParsedTempToken {
        actual_token,
        site_code,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JwtResponse {
    status: bool,
    user_info: Option<UserInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserInfo {
    access_token: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    user_id: String,
}

#[derive(Debug, Deserialize)]
struct TeamResponse {
    #[serde(default)]
    teams: Vec<Team>,
}

#[derive(Debug, Deserialize)]
struct Team {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
}

fn jwt_user_id(jwt: &str) -> Result<String, AuthError> {
    let payload = jwt.split('.').nth(1).ok_or(AuthError::InvalidJwt)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| AuthError::InvalidJwt)?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| AuthError::InvalidJwt)?;
    value
        .get("userId")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or(AuthError::InvalidJwt)
}

struct CallbackReceiver {
    listener: TcpListener,
    port: u16,
    client_secret: String,
}

impl CallbackReceiver {
    fn bind(preferred_port: u16) -> Result<Self, AuthError> {
        let ports = std::iter::once(preferred_port).chain(20_000..=20_100);
        for port in ports {
            match TcpListener::bind(("127.0.0.1", port)) {
                Ok(listener) => {
                    listener.set_nonblocking(true)?;
                    return Ok(Self {
                        listener,
                        port,
                        client_secret: random_hex(16),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(AuthError::Io(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            "no callback port available",
        )))
    }

    fn port(&self) -> u16 {
        self.port
    }

    fn client_secret(&self) -> &str {
        &self.client_secret
    }

    fn wait_for_token(&self, timeout: Duration) -> Result<String, AuthError> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(token) = handle_callback(stream)? {
                        return Ok(token);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(100));
                }
                Err(error) => return Err(error.into()),
            }
        }
        Err(AuthError::Timeout)
    }
}

fn handle_callback(mut stream: TcpStream) -> Result<Option<String>, AuthError> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let request = read_request(&mut stream)?;
    let token = extract_callback_token(&request);
    let body = b"<html><body><h2>Login successful. You can close this tab.</h2><script>window.close()</script></body></html>";
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    Ok(token)
}

fn read_request(stream: &mut TcpStream) -> Result<Vec<u8>, std::io::Error> {
    let mut request = Vec::with_capacity(2048);
    let mut buffer = [0_u8; 2048];
    loop {
        let size = stream.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..size]);
        if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
            let header_end = header_end + 4;
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| line.split_once(':'))
                .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if request.len() >= header_end + content_length {
                break;
            }
        }
        if request.len() > 64 * 1024 {
            break;
        }
    }
    Ok(request)
}

fn extract_callback_token(request: &[u8]) -> Option<String> {
    let request = String::from_utf8_lossy(request);
    let (headers, body) = request.split_once("\r\n\r\n").unwrap_or((&request, ""));
    let request_line = headers.lines().next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    if method.eq_ignore_ascii_case("GET") {
        let url = Url::parse(&format!("http://localhost{target}")).ok()?;
        if let Some((_, token)) = url.query_pairs().find(|(name, _)| name == "tempToken") {
            return Some(token.into_owned());
        }
        return url
            .query()
            .filter(|query| query.len() > 10)
            .map(str::to_owned);
    }
    if method.eq_ignore_ascii_case("POST") {
        if let Some((_, token)) =
            url::form_urlencoded::parse(body.as_bytes()).find(|(name, _)| name == "tempToken")
        {
            return Some(token.into_owned());
        }
        return (!body.is_empty()).then(|| body.to_owned());
    }
    None
}

fn browser_launch_error(message: &str) -> AuthError {
    AuthError::Browser(redact_embedded_urls(message))
}

fn random_hex(length: usize) -> String {
    let bytes = (0..length).map(|_| rand::random::<u8>());
    let mut output = String::with_capacity(length * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::browser_launch_error;

    #[test]
    fn browser_launcher_error_hides_callback_secret() {
        let error = browser_launch_error(
            r#"launcher [\"open\", \"https://cn.devecostudio.huawei.com/console/DevEcoIDE/apply?port=10101&appid=1007&code=callback-secret\"] failed"#,
        )
        .to_string();
        assert!(error.contains("https://cn.devecostudio.huawei.com/<redacted>"));
        for secret in ["callback-secret", "code=", "port=10101", "appid=1007"] {
            assert!(!error.contains(secret), "leaked {secret:?}: {error}");
        }
    }
}

//! Entra ID device-code sign-in. Refresh tokens live in the keyring only.

use serde_json::{Value, json};
use std::thread;
use std::time::{Duration, Instant};

use super::http::{Transport, bearer, expect_ok, json_body, parse_json};
use super::keys::{self, scrub_secrets};

const GRAPH_SCOPE: &str = "https://graph.microsoft.com/.default offline_access openid profile";

#[derive(Clone, Debug)]
pub struct AuthConfig {
    pub tenant_id: String,
    pub client_id: String,
}

impl AuthConfig {
    pub fn from_settings(tenant_id: &str, client_id: &str) -> Result<Self, String> {
        let tenant_id = tenant_id.trim().to_string();
        let client_id = client_id.trim().to_string();
        if tenant_id.is_empty() {
            return Err(crate::lang::text("planner.need_tenant").into());
        }
        if client_id.is_empty() {
            return Err(crate::lang::text("planner.need_client").into());
        }
        if tenant_id.chars().any(|c| c.is_control() || c.is_whitespace()) || client_id.chars().any(|c| c.is_control() || c.is_whitespace())
        {
            return Err(crate::lang::text("planner.bad_ids").into());
        }
        Ok(Self { tenant_id, client_id })
    }

    fn token_url(&self) -> String {
        format!("https://login.microsoftonline.com/{}/oauth2/v2.0/token", self.tenant_id)
    }

    fn device_url(&self) -> String {
        format!("https://login.microsoftonline.com/{}/oauth2/v2.0/devicecode", self.tenant_id)
    }
}

#[derive(Clone, Debug)]
pub struct DeviceCode {
    pub user_code: String,
    pub verification_uri: String,
    pub message: String,
    pub device_code: String,
    pub interval_secs: u64,
    pub expires_in: u64,
}

#[derive(Clone, Debug)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Seconds until the access token expires (from the token response).
    #[allow(dead_code)]
    pub expires_in: u64,
}

/// Starts the device-code flow. The caller shows `user_code` / `verification_uri` then polls.
pub fn begin_device_code(http: &mut dyn Transport, cfg: &AuthConfig, scope: &str) -> Result<DeviceCode, String> {
    let secret = keys::load_client_secret()?.unwrap_or_default();
    let mut form = format!("client_id={}&scope={}", urlencoding(cfg.client_id.as_str()), urlencoding(scope));
    if !secret.is_empty() {
        form.push_str(&format!("&client_secret={}", urlencoding(&secret)));
    }
    let resp = http.request("POST", &cfg.device_url(), &[("Content-Type", "application/x-www-form-urlencoded")], Some(&form))?;
    let secrets = [secret.as_str()];
    expect_ok(&resp, &secrets, "device code")?;
    let value = parse_json(&resp.body)?;
    let user_code = text(&value, "user_code")?;
    let device_code = text(&value, "device_code")?;
    let verification_uri = value
        .get("verification_uri")
        .or_else(|| value.get("verification_url"))
        .and_then(|v| v.as_str())
        .unwrap_or("https://microsoft.com/devicelogin")
        .to_string();
    let message = value.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let interval_secs = value.get("interval").and_then(|v| v.as_u64()).unwrap_or(5).max(1);
    let expires_in = value.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(900);
    Ok(DeviceCode { user_code, verification_uri, message, device_code, interval_secs, expires_in })
}

/// Polls until the user finishes device login, then stores the refresh token in the keyring.
pub fn poll_device_code(http: &mut dyn Transport, cfg: &AuthConfig, pending: &DeviceCode) -> Result<TokenSet, String> {
    let deadline = Instant::now() + Duration::from_secs(pending.expires_in.max(30));
    let secret = keys::load_client_secret()?.unwrap_or_default();
    loop {
        if Instant::now() >= deadline {
            return Err(crate::lang::text("planner.auth_timeout").into());
        }
        thread::sleep(Duration::from_secs(pending.interval_secs.max(1)));
        let mut form = format!(
            "grant_type=urn:ietf:params:oauth:grant-type:device_code&client_id={}&device_code={}",
            urlencoding(&cfg.client_id),
            urlencoding(&pending.device_code)
        );
        if !secret.is_empty() {
            form.push_str(&format!("&client_secret={}", urlencoding(&secret)));
        }
        let resp = http.request("POST", &cfg.token_url(), &[("Content-Type", "application/x-www-form-urlencoded")], Some(&form))?;
        if resp.status == 200 {
            let tokens = tokens_from(&resp.body, &[secret.as_str(), pending.device_code.as_str()])?;
            persist_refresh(&tokens)?;
            return Ok(tokens);
        }
        let err = parse_json(&resp.body).ok().and_then(|v| text(&v, "error").ok()).unwrap_or_default();
        match err.as_str() {
            "authorization_pending" => continue,
            "slow_down" => {
                thread::sleep(Duration::from_secs(pending.interval_secs.saturating_add(2)));
            }
            "authorization_declined" | "expired_token" | "bad_verification_code" => {
                return Err(scrub_secrets(&format!("sign-in failed ({err})"), &[secret.as_str(), pending.device_code.as_str()]));
            }
            _ => {
                let detail = resp.body.chars().take(200).collect::<String>();
                return Err(scrub_secrets(
                    &format!("sign-in failed: HTTP {} — {detail}", resp.status),
                    &[secret.as_str(), pending.device_code.as_str()],
                ));
            }
        }
    }
}

fn uses_azure_cli(cfg: &AuthConfig) -> bool {
    super::az_cli::using_az() || cfg.client_id == super::az_cli::PLACEHOLDER_CLIENT_ID
}

/// Uses Azure CLI when that was the sign-in path or no client id is configured; otherwise a stored refresh token.
pub fn graph_token(http: &mut dyn Transport, cfg: &AuthConfig) -> Result<TokenSet, String> {
    if uses_azure_cli(cfg) {
        return super::az_cli::graph_token();
    }
    refresh_for_scope(http, cfg, GRAPH_SCOPE)
}

pub fn dataverse_token(http: &mut dyn Transport, cfg: &AuthConfig, org_url: &str) -> Result<TokenSet, String> {
    let org = org_url.trim().trim_end_matches('/');
    if org.is_empty() || !org.starts_with("https://") {
        return Err(crate::lang::text("planner.need_org").into());
    }
    if uses_azure_cli(cfg) {
        return super::az_cli::dataverse_token(org);
    }
    let scope = format!("{org}/.default offline_access");
    refresh_for_scope(http, cfg, &scope)
}

fn refresh_for_scope(http: &mut dyn Transport, cfg: &AuthConfig, scope: &str) -> Result<TokenSet, String> {
    let refresh = keys::load_refresh_token()?.ok_or_else(|| crate::lang::text("planner.need_sign_in").to_string())?;
    let secret = keys::load_client_secret()?.unwrap_or_default();
    let mut form = format!(
        "grant_type=refresh_token&client_id={}&refresh_token={}&scope={}",
        urlencoding(&cfg.client_id),
        urlencoding(&refresh),
        urlencoding(scope)
    );
    if !secret.is_empty() {
        form.push_str(&format!("&client_secret={}", urlencoding(&secret)));
    }
    let resp = http.request("POST", &cfg.token_url(), &[("Content-Type", "application/x-www-form-urlencoded")], Some(&form))?;
    let secrets = [refresh.as_str(), secret.as_str()];
    if resp.status != 200 {
        let _ = keys::delete_refresh_token();
        expect_ok(&resp, &secrets, "token refresh")?;
    }
    let tokens = tokens_from(&resp.body, &secrets)?;
    persist_refresh(&tokens)?;
    Ok(tokens)
}

fn persist_refresh(tokens: &TokenSet) -> Result<(), String> {
    if let Some(refresh) = &tokens.refresh_token {
        keys::store_refresh_token(refresh)?;
    }
    Ok(())
}

fn tokens_from(body: &str, extra_secrets: &[&str]) -> Result<TokenSet, String> {
    let value = parse_json(body)?;
    let access_token = text(&value, "access_token").map_err(|e| scrub_secrets(&e, extra_secrets))?;
    let refresh_token = value.get("refresh_token").and_then(|v| v.as_str()).map(str::to_string);
    let expires_in = value.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(3600);
    Ok(TokenSet { access_token, refresh_token, expires_in })
}

fn text(value: &Value, key: &str) -> Result<String, String> {
    value.get(key).and_then(|v| v.as_str()).map(str::to_string).filter(|s| !s.is_empty()).ok_or_else(|| format!("missing {key}"))
}

/// Minimal form encoding for OAuth bodies (ASCII identifiers and tokens).
pub fn urlencoding(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// Graph GET helper used by export steps.
pub fn graph_get(http: &mut dyn Transport, tokens: &TokenSet, path: &str) -> Result<Value, String> {
    let url = if path.starts_with("https://") { path.to_string() } else { format!("https://graph.microsoft.com{path}") };
    let auth = bearer(&tokens.access_token);
    let resp = http.request("GET", &url, &[("Authorization", &auth)], None)?;
    expect_ok(&resp, &[tokens.access_token.as_str()], "Graph")?;
    parse_json(&resp.body)
}

pub fn graph_post(http: &mut dyn Transport, tokens: &TokenSet, path: &str, body: &Value) -> Result<Value, String> {
    let url = format!("https://graph.microsoft.com{path}");
    let auth = bearer(&tokens.access_token);
    let payload = json_body(body)?;
    let resp = http.request("POST", &url, &[("Authorization", &auth), ("Content-Type", "application/json")], Some(&payload))?;
    expect_ok(&resp, &[tokens.access_token.as_str()], "Graph")?;
    if resp.body.trim().is_empty() {
        return Ok(json!({}));
    }
    parse_json(&resp.body)
}

pub fn graph_patch(http: &mut dyn Transport, tokens: &TokenSet, path: &str, etag: &str, body: &Value) -> Result<(), String> {
    let url = format!("https://graph.microsoft.com{path}");
    let auth = bearer(&tokens.access_token);
    let payload = json_body(body)?;
    let resp =
        http.request("PATCH", &url, &[("Authorization", &auth), ("Content-Type", "application/json"), ("If-Match", etag)], Some(&payload))?;
    expect_ok(&resp, &[tokens.access_token.as_str()], "Graph")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner_export::http::HttpResponse;
    use crate::planner_export::http::mock::Scripted;

    #[test]
    fn urlencoding_keeps_safe_chars() {
        assert_eq!(urlencoding("abc-_.~"), "abc-_.~");
        assert_eq!(urlencoding("a b"), "a+b");
        assert!(urlencoding("a/b").contains("%2F"));
    }

    #[test]
    fn auth_config_rejects_blank() {
        assert!(AuthConfig::from_settings("", "client").is_err());
        assert!(AuthConfig::from_settings("tenant", "").is_err());
        assert!(AuthConfig::from_settings("ten ant", "client").is_err());
    }

    #[test]
    fn begin_device_code_reads_payload() {
        let mut http = Scripted::new(vec![Ok(HttpResponse {
            status: 200,
            body: r#"{"user_code":"ABCD","device_code":"dev","verification_uri":"https://example/device","interval":5,"expires_in":600,"message":"Go"}"#.into(),
        })]);
        let cfg = AuthConfig { tenant_id: "t".into(), client_id: "c".into() };
        let pending = begin_device_code(&mut http, &cfg, GRAPH_SCOPE).unwrap();
        assert_eq!(pending.user_code, "ABCD");
        assert_eq!(pending.verification_uri, "https://example/device");
    }
}

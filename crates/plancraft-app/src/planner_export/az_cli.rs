//! Sign in via Azure CLI (`az login`) so users need no PlanCraft Entra app id.
//!
//! Tokens are obtained with `az account get-access-token`. Do not run interactive
//! login from CI or machines where Microsoft login is blocked — only from the
//! user's machine where a browser login works.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::auth::{DeviceCode, TokenSet};
use super::keys;

const AUTH_MODE_AZ: &str = "az";

/// True when the last successful Planner sign-in used Azure CLI.
pub fn using_az() -> bool {
    keys::load_auth_mode().ok().flatten().as_deref() == Some(AUTH_MODE_AZ)
}

pub fn mark_az_signed_in() -> Result<(), String> {
    keys::store_auth_mode(AUTH_MODE_AZ)
}

pub fn clear_az() -> Result<(), String> {
    keys::delete_auth_mode()
}

/// Graph access token via Azure CLI (already logged in).
pub fn graph_token() -> Result<TokenSet, String> {
    access_token("https://graph.microsoft.com")
}

/// Dataverse / org access token via Azure CLI.
pub fn dataverse_token(org_url: &str) -> Result<TokenSet, String> {
    let org = org_url.trim().trim_end_matches('/');
    if org.is_empty() || !org.starts_with("https://") {
        return Err(crate::lang::text("planner.need_org").into());
    }
    access_token(org)
}

fn access_token(resource: &str) -> Result<TokenSet, String> {
    let output = Command::new("az")
        .args(["account", "get-access-token", "--resource", resource, "-o", "json"])
        .output()
        .map_err(|_| crate::lang::text("planner.az_missing").to_string())?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let brief = err.chars().take(240).collect::<String>();
        return Err(format!("{} ({brief})", crate::lang::text("planner.az_not_signed_in")));
    }
    let value: Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    let access_token = value
        .get("accessToken")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "az: missing accessToken".to_string())?
        .to_string();
    let expires_in = value.get("expiresOn").and_then(|_| Some(3600u64)).unwrap_or(3600);
    Ok(TokenSet { access_token, refresh_token: None, expires_in })
}

/// Start `az login --use-device-code` and parse the device code for our UI.
pub fn begin_device_login() -> Result<(DeviceCode, mpsc::Receiver<Result<(), String>>), String> {
    let mut child = Command::new("az")
        .args(["login", "--use-device-code"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| crate::lang::text("planner.az_missing").to_string())?;

    let stdout = child.stdout.take().ok_or_else(|| "az: no stdout".to_string())?;
    let stderr = child.stderr.take().ok_or_else(|| "az: no stderr".to_string())?;

    let (code_tx, code_rx) = mpsc::channel::<Result<DeviceCode, String>>();
    let (done_tx, done_rx) = mpsc::channel::<Result<(), String>>();

    thread::spawn(move || {
        let mut combined = String::new();
        let out = BufReader::new(stdout);
        let err = BufReader::new(stderr);
        // Merge lines from both pipes until we can parse a device code, then wait for exit.
        let (line_tx, line_rx) = mpsc::channel::<String>();
        let line_tx2 = line_tx.clone();
        thread::spawn(move || {
            for line in out.lines().flatten() {
                let _ = line_tx.send(line);
            }
        });
        thread::spawn(move || {
            for line in err.lines().flatten() {
                let _ = line_tx2.send(line);
            }
        });

        let deadline = Instant::now() + Duration::from_secs(120);
        let mut sent_code = false;
        while Instant::now() < deadline {
            match line_rx.recv_timeout(Duration::from_millis(200)) {
                Ok(line) => {
                    combined.push_str(&line);
                    combined.push('\n');
                    if !sent_code {
                        if let Some(pending) = parse_az_device_code(&combined) {
                            let _ = code_tx.send(Ok(pending));
                            sent_code = true;
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(status) = child.try_wait().ok().flatten() {
                        if !sent_code {
                            let _ = code_tx.send(Err(format!(
                                "{} — {}",
                                crate::lang::text("planner.az_login_failed"),
                                combined.chars().take(200).collect::<String>()
                            )));
                        }
                        let _ = done_tx.send(if status.success() {
                            Ok(())
                        } else {
                            Err(crate::lang::text("planner.az_login_failed").into())
                        });
                        return;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        if !sent_code {
            let _ = code_tx.send(Err(crate::lang::text("planner.az_login_failed").into()));
        }
        match child.wait() {
            Ok(status) if status.success() => {
                let _ = done_tx.send(Ok(()));
            }
            Ok(_) => {
                let _ = done_tx.send(Err(crate::lang::text("planner.az_login_failed").into()));
            }
            Err(e) => {
                let _ = done_tx.send(Err(e.to_string()));
            }
        }
    });

    let pending = code_rx.recv_timeout(Duration::from_secs(90)).map_err(|_| crate::lang::text("planner.az_login_failed").to_string())??;
    Ok((pending, done_rx))
}

fn parse_az_device_code(text: &str) -> Option<DeviceCode> {
    // Typical: "open the page https://microsoft.com/devicelogin and enter the code ABCD-EFGH"
    let lower = text.to_lowercase();
    let uri = [
        "https://microsoft.com/devicelogin",
        "https://aka.ms/devicelogin",
        "https://login.microsoft.com/device",
        "https://login.microsoftonline.com/device",
    ]
    .into_iter()
    .find(|u| lower.contains(&u.to_lowercase()))
    .unwrap_or("https://microsoft.com/devicelogin")
    .to_string();

    // Prefer codes that look like XXXX-XXXX
    let mut code = None;
    for token in text.split_whitespace() {
        let t = token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-');
        if t.len() >= 8 && t.len() <= 15 && t.contains('-') && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            code = Some(t.to_uppercase());
            break;
        }
    }
    let user_code = code?;
    Some(DeviceCode {
        user_code: user_code.clone(),
        verification_uri: uri,
        message: format!("To sign in, open the page and enter the code {user_code}"),
        device_code: String::new(),
        interval_secs: 3,
        expires_in: 900,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_az_device_message() {
        let sample = "To sign in, use a web browser to open the page https://microsoft.com/devicelogin and enter the code ABCD-EFGH to authenticate.";
        let pending = parse_az_device_code(sample).unwrap();
        assert_eq!(pending.user_code, "ABCD-EFGH");
        assert!(pending.verification_uri.contains("devicelogin"));
    }
}

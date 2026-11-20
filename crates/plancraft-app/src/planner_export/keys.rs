//! Planner tokens and optional client secrets in the OS key store. Never written to settings.json.

use keyring::Entry;

use crate::distill::scrub;

const SERVICE: &str = "plancraft";
const REFRESH: &str = "planner.refresh_token";
const CLIENT_SECRET: &str = "planner.client_secret";

fn entry(account: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, account).map_err(|e| e.to_string())
}

fn load(account: &str) -> Result<Option<String>, String> {
    match entry(account)?.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn store(account: &str, secret: &str, max_chars: usize) -> Result<(), String> {
    let secret = secret.trim();
    if secret.is_empty() {
        return Err(crate::lang::text("distill.paste").into());
    }
    if secret.chars().count() > max_chars || secret.chars().any(|c| c.is_control()) {
        return Err(crate::lang::text("distill.key_rejected").into());
    }
    entry(account)?.set_password(secret).map_err(|e| scrub(&e.to_string(), secret))
}

fn delete(account: &str) -> Result<(), String> {
    match entry(account)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn load_refresh_token() -> Result<Option<String>, String> {
    load(REFRESH)
}

pub fn store_refresh_token(token: &str) -> Result<(), String> {
    // Entra refresh tokens are long; Distill's 500-char cap does not apply here.
    store(REFRESH, token, 8_000)
}

pub fn delete_refresh_token() -> Result<(), String> {
    delete(REFRESH)
}

pub fn load_client_secret() -> Result<Option<String>, String> {
    load(CLIENT_SECRET)
}

pub fn store_client_secret(secret: &str) -> Result<(), String> {
    store(CLIENT_SECRET, secret, 500)
}

pub fn delete_client_secret() -> Result<(), String> {
    delete(CLIENT_SECRET)
}

pub fn scrub_secrets(message: &str, secrets: &[&str]) -> String {
    let mut out = message.to_string();
    for secret in secrets {
        if !secret.is_empty() {
            out = scrub(&out, secret);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_hides_every_secret() {
        let msg = scrub_secrets("a=tok1 b=tok2", &["tok1", "tok2"]);
        assert!(!msg.contains("tok1"));
        assert!(!msg.contains("tok2"));
        assert!(msg.contains("••••"));
    }
}

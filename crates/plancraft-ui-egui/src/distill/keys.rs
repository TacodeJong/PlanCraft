//! API keys in the operating system's store. Nothing here is written to settings.json.

use keyring::Entry;

const SERVICE: &str = "plancraft";

fn entry(account: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, account).map_err(|e| e.to_string())
}

/// The saved key, or nothing when this service has no key yet.
pub fn load(account: &str) -> Result<Option<String>, String> {
    match entry(account)?.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn store(account: &str, secret: &str) -> Result<(), String> {
    let secret = secret.trim();
    if secret.is_empty() {
        return Err(crate::lang::text("distill.paste").into());
    }
    if secret.chars().count() > 500 || secret.chars().any(|c| c.is_control()) {
        return Err(crate::lang::text("distill.key_rejected").into());
    }
    entry(account)?.set_password(secret).map_err(|e| scrub(&e.to_string(), secret))
}

pub fn delete(account: &str) -> Result<(), String> {
    match entry(account)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// The last four characters, for a "key saved" line. The rest of the key is not returned.
pub fn tail(secret: &str) -> String {
    let n = secret.chars().count();
    secret.chars().skip(n.saturating_sub(4)).collect()
}

/// Removes `secret` from a message before it is shown or logged.
pub fn scrub(message: &str, secret: &str) -> String {
    if secret.is_empty() {
        return message.to_string();
    }
    message.replace(secret, "••••")
}

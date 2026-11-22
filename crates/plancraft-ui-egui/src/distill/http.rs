//! One chat completion. Tests supply their own [`Chat`]; the app uses [`UreqChat`].

use std::io::Read;
use std::time::Duration;

use serde_json::{Value, json};

use crate::distill::keys::scrub;
use crate::settings::AiProvider;

const MAX_REPLY: usize = 1_000_000;

/// What one completion needs. `Debug` leaves the key out.
pub struct ChatRequest {
    pub provider: AiProvider,
    pub model: String,
    pub base_url: String,
    pub api_key: String,
    pub system: String,
    pub user: String,
}

impl std::fmt::Debug for ChatRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatRequest")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

pub trait Chat {
    fn complete(&mut self, request: &ChatRequest) -> Result<String, String>;
}

pub struct UreqChat;

impl Chat for UreqChat {
    fn complete(&mut self, request: &ChatRequest) -> Result<String, String> {
        let url = endpoint(request.provider, &request.base_url)?;
        let body = body_for(request);
        let payload = serde_json::to_string(&body).map_err(|e| e.to_string())?;
        let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout(Duration::from_secs(90)).build();
        let mut call = agent.post(&url).set("Content-Type", "application/json");
        call = match request.provider {
            AiProvider::Anthropic => call.set("x-api-key", &request.api_key).set("anthropic-version", "2023-06-01"),
            _ => call.set("Authorization", &format!("Bearer {}", request.api_key)),
        };
        let reply = match call.send_string(&payload) {
            Ok(response) => read_limited(response)?,
            Err(ureq::Error::Status(code, response)) => {
                let text = read_limited(response).unwrap_or_default();
                let detail = error_message(&text);
                return Err(scrub(&format!("the service returned HTTP {code}: {detail}"), &request.api_key));
            }
            Err(ureq::Error::Transport(err)) => {
                return Err(scrub(&format!("{}: {err}", crate::lang::text("distill.offline")), &request.api_key));
            }
        };
        let value: Value = serde_json::from_str(&reply).map_err(|_| crate::lang::text("distill.not_json").to_string())?;
        if let Some(msg) = error_message_value(&value) {
            return Err(scrub(&msg, &request.api_key));
        }
        let text = match request.provider {
            AiProvider::Anthropic => anthropic_text(&value),
            _ => openai_text(&value),
        }?;
        Ok(scrub(&text, &request.api_key))
    }
}

pub fn endpoint(provider: AiProvider, base: &str) -> Result<String, String> {
    match provider {
        AiProvider::OpenAi => Ok("https://api.openai.com/v1/chat/completions".into()),
        AiProvider::Xai => Ok("https://api.x.ai/v1/chat/completions".into()),
        AiProvider::Anthropic => Ok("https://api.anthropic.com/v1/messages".into()),
        AiProvider::Compatible => compatible_url(base),
    }
}

fn compatible_url(base: &str) -> Result<String, String> {
    let base = base.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err(crate::lang::text("distill.need_url").into());
    }
    if base.contains('@') || base.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(crate::lang::text("distill.bad_url").into());
    }
    let https = base.starts_with("https://");
    let http = base.starts_with("http://");
    if !https && !http {
        return Err(crate::lang::text("distill.bad_url").into());
    }
    if base.ends_with("/chat/completions") { Ok(base.to_string()) } else { Ok(format!("{base}/chat/completions")) }
}

fn body_for(request: &ChatRequest) -> Value {
    match request.provider {
        AiProvider::Anthropic => json!({
            "model": request.model,
            "max_tokens": 4096,
            "system": request.system,
            "messages": [{"role": "user", "content": request.user}]
        }),
        AiProvider::OpenAi | AiProvider::Xai => json!({
            "model": request.model,
            "response_format": {"type": "json_object"},
            "messages": [
                {"role": "system", "content": request.system},
                {"role": "user", "content": request.user}
            ]
        }),
        AiProvider::Compatible => json!({
            "model": request.model,
            "messages": [
                {"role": "system", "content": request.system},
                {"role": "user", "content": request.user}
            ]
        }),
    }
}

fn openai_text(value: &Value) -> Result<String, String> {
    let content = value.get("choices").and_then(|c| c.get(0)).and_then(|c| c.get("message")).and_then(|m| m.get("content"));
    text_of(content)
}

fn anthropic_text(value: &Value) -> Result<String, String> {
    text_of(value.get("content"))
}

fn text_of(content: Option<&Value>) -> Result<String, String> {
    match content {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.clone()),
        Some(Value::Array(parts)) => {
            let mut out = String::new();
            for part in parts {
                if let Some(text) = part.get("text").and_then(|t| t.as_str()).or_else(|| part.as_str()) {
                    out.push_str(text);
                }
            }
            if out.trim().is_empty() { Err(crate::lang::text("distill.empty").into()) } else { Ok(out) }
        }
        _ => Err(crate::lang::text("distill.empty").into()),
    }
}

fn error_message_value(value: &Value) -> Option<String> {
    value.get("error").and_then(|e| e.get("message").and_then(|m| m.as_str()).or_else(|| e.as_str())).map(str::to_string)
}

fn error_message(text: &str) -> String {
    serde_json::from_str::<Value>(text).ok().as_ref().and_then(error_message_value).unwrap_or_else(|| text.chars().take(300).collect())
}

fn read_limited(response: ureq::Response) -> Result<String, String> {
    let mut reader = response.into_reader();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len().saturating_add(n) > MAX_REPLY {
                    return Err(crate::lang::text("distill.too_large").into());
                }
                if let Some(part) = chunk.get(..n) {
                    buf.extend_from_slice(part);
                }
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    String::from_utf8(buf).map_err(|_| crate::lang::text("distill.not_json").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_body_and_debug_leave_the_key_out() {
        let request = ChatRequest {
            provider: AiProvider::OpenAi,
            model: "gpt-4o-mini".into(),
            base_url: String::new(),
            api_key: "sk-secret-value".into(),
            system: "reply with json".into(),
            user: "a brief".into(),
        };
        let body = body_for(&request).to_string();
        assert!(!body.contains("sk-secret-value"), "{body}");
        let shown = format!("{request:?}");
        assert!(!shown.contains("sk-secret-value"), "{shown}");
        assert_eq!(endpoint(AiProvider::OpenAi, "").unwrap(), "https://api.openai.com/v1/chat/completions");
        assert!(compatible_url("http://127.0.0.1:11434/v1").unwrap().ends_with("/chat/completions"));
        assert!(compatible_url("not a url").is_err());
    }
}

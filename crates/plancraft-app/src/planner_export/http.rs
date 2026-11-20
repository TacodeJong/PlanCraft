//! Shared HTTP helpers for Graph and Dataverse. Secrets are scrubbed from errors.

use std::io::Read;
use std::time::Duration;

use serde_json::Value;

use super::keys::scrub_secrets;

const MAX_BODY: usize = 4_000_000;

pub trait Transport {
    fn request(&mut self, method: &str, url: &str, headers: &[(&str, &str)], body: Option<&str>) -> Result<HttpResponse, String>;
}

#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

pub struct UreqTransport {
    agent: ureq::Agent,
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self { agent: ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout(Duration::from_secs(120)).build() }
    }
}

impl Transport for UreqTransport {
    fn request(&mut self, method: &str, url: &str, headers: &[(&str, &str)], body: Option<&str>) -> Result<HttpResponse, String> {
        let mut call = self.agent.request(method, url);
        for (k, v) in headers {
            call = call.set(k, v);
        }
        let result = match body {
            Some(payload) => call.send_string(payload),
            None => call.call(),
        };
        match result {
            Ok(response) => {
                let status = response.status();
                let body = read_limited(response)?;
                Ok(HttpResponse { status, body })
            }
            Err(ureq::Error::Status(code, response)) => {
                let body = read_limited(response).unwrap_or_default();
                Ok(HttpResponse { status: code, body })
            }
            Err(ureq::Error::Transport(err)) => Err(format!("{}: {err}", crate::lang::text("planner.offline"))),
        }
    }
}

fn read_limited(response: ureq::Response) -> Result<String, String> {
    let mut reader = response.into_reader();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len().saturating_add(n) > MAX_BODY {
                    return Err(crate::lang::text("planner.too_large").into());
                }
                if let Some(part) = chunk.get(..n) {
                    buf.extend_from_slice(part);
                }
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    String::from_utf8(buf).map_err(|_| crate::lang::text("planner.not_json").to_string())
}

pub fn json_body(value: &Value) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}

pub fn parse_json(body: &str) -> Result<Value, String> {
    serde_json::from_str(body).map_err(|_| crate::lang::text("planner.not_json").to_string())
}

pub fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

pub fn expect_ok(resp: &HttpResponse, secrets: &[&str], what: &str) -> Result<(), String> {
    if (200..300).contains(&resp.status) {
        return Ok(());
    }
    let detail = error_detail(&resp.body);
    Err(scrub_secrets(&format!("{what}: HTTP {} — {detail}", resp.status), secrets))
}

fn error_detail(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(body)
        && let Some(msg) = value
            .pointer("/error/message")
            .and_then(|v| v.as_str())
            .or_else(|| value.get("error_description").and_then(|v| v.as_str()))
            .or_else(|| value.get("message").and_then(|v| v.as_str()))
    {
        return msg.chars().take(400).collect();
    }
    body.chars().take(300).collect()
}

#[cfg(test)]
pub mod mock {
    use super::*;
    use std::collections::VecDeque;

    pub struct Scripted {
        pub calls: Vec<(String, String, Option<String>)>,
        pub responses: VecDeque<Result<HttpResponse, String>>,
    }

    impl Scripted {
        pub fn new(responses: Vec<Result<HttpResponse, String>>) -> Self {
            Self { calls: Vec::new(), responses: responses.into() }
        }
    }

    impl Transport for Scripted {
        fn request(&mut self, method: &str, url: &str, _headers: &[(&str, &str)], body: Option<&str>) -> Result<HttpResponse, String> {
            self.calls.push((method.into(), url.into(), body.map(str::to_string)));
            self.responses.pop_front().unwrap_or_else(|| Err("no scripted response".into()))
        }
    }
}

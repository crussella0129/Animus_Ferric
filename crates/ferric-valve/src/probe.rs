//! Startup enforcement probe (T-12607, INT-0012 AC-6).
//!
//! The valve must never serve unconstrained results under a constrained label.
//! Before serving constrained mode it asks the upstream for the word "hello"
//! while constraining the reply to `{"ok":"yes"}`. Only an upstream that
//! actually enforces the schema can answer with that object. A backend that
//! ignores `response_format` answers in prose, and the valve refuses to start.

use serde_json::{Value, json};

/// The one object the probe schema admits.
pub fn probe_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "ok": { "enum": ["yes"] } },
        "required": ["ok"],
        "additionalProperties": false
    })
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ProbeError {
    #[error("upstream unreachable: {0}")]
    Unreachable(String),
    #[error("upstream returned HTTP {status}: {excerpt}")]
    Http { status: u16, excerpt: String },
    #[error("upstream did not enforce the probe schema; it replied {0:?}")]
    NotEnforced(String),
}

/// The probe request body.
pub fn probe_body() -> Value {
    json!({
        "model": "ferric-valve-probe",
        "messages": [{ "role": "user", "content": "Reply with the single word: hello" }],
        "max_tokens": 32,
        "temperature": 0,
        "stream": false,
        "chat_template_kwargs": { "enable_thinking": false },
        "response_format": {
            "type": "json_schema",
            "json_schema": { "name": "ferric_probe", "schema": probe_schema(), "strict": true }
        }
    })
}

/// `Ok` only if the upstream's reply is exactly `{"ok":"yes"}`.
pub async fn probe_enforcement(
    client: &reqwest::Client,
    upstream_base: &str,
) -> Result<(), ProbeError> {
    let url = format!(
        "{}/v1/chat/completions",
        upstream_base.trim_end_matches('/')
    );
    let response = client
        .post(url)
        .json(&probe_body())
        .send()
        .await
        .map_err(|error| ProbeError::Unreachable(error.to_string()))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(ProbeError::Http {
            status: status.as_u16(),
            excerpt: text.chars().take(512).collect(),
        });
    }
    let content = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|body| {
            body.pointer("/choices/0/message/content")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();
    match serde_json::from_str::<Value>(content.trim()) {
        Ok(value) if value == json!({"ok": "yes"}) => Ok(()),
        _ => Err(ProbeError::NotEnforced(content)),
    }
}

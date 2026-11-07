//! The downstream server-sent-event writer (T-12606). Upstream SSE is decoded
//! by `ferric_iron::wire::SseDecoder`, shared with Ferric's provider.

use serde_json::{Value, json};

/// One SSE frame carrying `value`.
pub fn frame(value: &Value) -> String {
    format!("data: {value}\n\n")
}

/// The stream terminator frame.
pub fn done_frame() -> String {
    "data: [DONE]\n\n".to_string()
}

/// Identity fields every chunk of one response shares.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkIdentity {
    pub id: String,
    pub created: u64,
    pub model: String,
}

/// A `chat.completion.chunk` with one choice.
pub fn chunk(identity: &ChunkIdentity, delta: Value, finish_reason: Option<&str>) -> Value {
    json!({
        "id": identity.id,
        "object": "chat.completion.chunk",
        "created": identity.created,
        "model": identity.model,
        "choices": [{ "index": 0, "delta": delta, "finish_reason": finish_reason }]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_well_formed() {
        let identity = ChunkIdentity {
            id: "chatcmpl-x".to_string(),
            created: 7,
            model: "m".to_string(),
        };
        let value = chunk(&identity, json!({"content": "hi"}), None);
        assert_eq!(value["choices"][0]["finish_reason"], Value::Null);
        assert_eq!(frame(&value), format!("data: {value}\n\n"));
        assert_eq!(done_frame(), "data: [DONE]\n\n");
    }
}

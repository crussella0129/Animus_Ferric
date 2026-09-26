//! Server-sent-event plumbing shared by the upstream reader and the downstream
//! writer (T-12606).

use serde_json::{Value, json};

/// Reassembles SSE lines from arbitrary byte chunks. A line split across two
/// network reads is only yielded once its terminator arrives.
#[derive(Debug, Default)]
pub struct SseLineBuffer {
    pending: Vec<u8>,
}

impl SseLineBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed bytes; return every complete line (without `\n` or `\r\n`).
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.pending.extend_from_slice(bytes);
        let mut lines = Vec::new();
        while let Some(end) = self.pending.iter().position(|&byte| byte == b'\n') {
            let mut line: Vec<u8> = self.pending.drain(..=end).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            lines.push(String::from_utf8_lossy(&line).into_owned());
        }
        lines
    }
}

/// What one SSE line carries.
#[derive(Debug, Clone, PartialEq)]
pub enum SseLine {
    /// A `data:` line holding JSON.
    Data(Value),
    /// `data: [DONE]`.
    Done,
    /// Blank lines, comments, other fields, or undecodable data.
    Other,
}

pub fn classify(line: &str) -> SseLine {
    let Some(payload) = line.strip_prefix("data:") else {
        return SseLine::Other;
    };
    let payload = payload.trim();
    if payload == "[DONE]" {
        return SseLine::Done;
    }
    serde_json::from_str(payload).map_or(SseLine::Other, SseLine::Data)
}

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
    fn sse_lines_split_across_reads() {
        let mut buffer = SseLineBuffer::new();
        assert!(buffer.push(b"data: {\"a\"").is_empty());
        assert_eq!(
            buffer.push(b":1}\r\n\r\ndata: [DO"),
            ["data: {\"a\":1}", ""]
        );
        assert_eq!(buffer.push(b"NE]\n"), ["data: [DONE]"]);
    }

    #[test]
    fn classify_recognizes_data_done_and_other() {
        assert_eq!(classify("data: {\"x\":1}"), SseLine::Data(json!({"x": 1})));
        assert_eq!(classify("data:[DONE]"), SseLine::Done);
        assert_eq!(classify(": keep-alive"), SseLine::Other);
        assert_eq!(classify(""), SseLine::Other);
        assert_eq!(classify("data: not json"), SseLine::Other);
    }

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

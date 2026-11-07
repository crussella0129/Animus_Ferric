//! The OpenAI chat-completions wire shapes both of Ferric's HTTP clients speak:
//! the provider's backend and the Hermes valve. Pure, with no HTTP client:
//! each keeps its own transport, and only the shapes and the SSE framing live
//! here, so the two cannot drift apart.

use serde_json::{Value, json};

use crate::types::ToolDescriptor;

/// What one server-sent-event line carries.
#[derive(Debug, Clone, PartialEq)]
pub enum SseLine {
    /// A `data:` line holding JSON.
    Data(Value),
    /// The `data: [DONE]` terminator.
    Done,
    /// Blank lines (frame separators), other SSE fields (`event:`, `id:`,
    /// `: comment`) and `data:` that is not JSON: not an error, just not a
    /// payload.
    Other,
}

/// Frames SSE in bytes before decoding UTF-8. A network chunk can end anywhere
/// within a code point; malformed or truncated bytes are errors, never
/// replacement characters silently inserted into content or tool arguments.
/// Nothing after `[DONE]` is decoded.
#[derive(Debug, Default)]
pub struct SseDecoder {
    pending: Vec<u8>,
    scanned: usize,
    done: bool,
}

impl SseDecoder {
    /// Feed bytes; return every line they complete, without its `\n` or
    /// `\r\n`.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<SseLine>, std::str::Utf8Error> {
        if self.done {
            return Ok(Vec::new());
        }
        self.pending.extend_from_slice(bytes);
        let mut lines = Vec::new();
        let mut consumed = 0;
        let mut search_from = self.scanned;
        while let Some(offset) = self.pending[search_from..].iter().position(|b| *b == b'\n') {
            let end = search_from + offset;
            let line = classify(std::str::from_utf8(&self.pending[consumed..end])?);
            let done = line == SseLine::Done;
            lines.push(line);
            consumed = end + 1;
            search_from = consumed;
            if done {
                self.pending.clear();
                self.scanned = 0;
                self.done = true;
                return Ok(lines);
            }
        }
        self.pending.drain(..consumed);
        // Do not rescan a growing incomplete line on every tiny TCP chunk.
        self.scanned = self.pending.len();
        Ok(lines)
    }

    /// End of stream. An unterminated final line is ignored, but a malformed
    /// or truncated UTF-8 tail is still an error.
    pub fn finish(self) -> Result<(), std::str::Utf8Error> {
        std::str::from_utf8(&self.pending).map(|_| ())
    }
}

fn classify(line: &str) -> SseLine {
    let Some(payload) = line.strip_prefix("data:") else {
        return SseLine::Other;
    };
    let payload = payload.trim();
    if payload == "[DONE]" {
        return SseLine::Done;
    }
    serde_json::from_str(payload).map_or(SseLine::Other, SseLine::Data)
}

/// A strict JSON-Schema `response_format` named `name`: the server-enforced
/// constraint a llama.cpp backend compiles into its token mask.
pub fn response_format(name: &str, schema: &Value) -> Value {
    json!({
        "type": "json_schema",
        "json_schema": { "name": name, "schema": schema, "strict": true }
    })
}

/// The OpenAI `tools` entry for `tool`, the inverse of
/// [`crate::descriptors_from_openai_tools`].
pub fn openai_tool(tool: &ToolDescriptor) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.input_schema
        }
    })
}

/// One assistant `tool_calls` entry; `arguments` is the JSON-encoded argument
/// object.
pub fn openai_tool_call(id: &str, name: &str, arguments: &str) -> Value {
    json!({
        "id": id,
        "type": "function",
        "function": { "name": name, "arguments": arguments }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_lines_split_across_reads() {
        let mut decoder = SseDecoder::default();
        assert!(decoder.push(b"data: {\"a\"").unwrap().is_empty());
        assert_eq!(
            decoder
                .push(b":1}\r\n\r\n: keep-alive\ndata: not json\ndata:[DO")
                .unwrap(),
            [
                SseLine::Data(json!({"a": 1})),
                SseLine::Other,
                SseLine::Other,
                SseLine::Other
            ]
        );
        assert_eq!(decoder.push(b"NE]\ndata: {}\n").unwrap(), [SseLine::Done]);
        decoder.finish().unwrap();
    }
}

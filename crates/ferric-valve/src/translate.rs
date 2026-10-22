//! Turning the constrained upstream completion back into an ordinary OpenAI
//! response (T-12606, INT-0012 AC-1/AC-2).
//!
//! Pure: fed upstream text, it yields the downstream deltas to emit now, and at
//! the end resolves the whole action. Streaming shows live signals only where
//! they cannot mislead:
//!
//! - the `thought` streams as `reasoning_content`;
//! - an offered tool's name streams as soon as the scanner commits to it.
//!
//! The final answer and the tool arguments are released only once the whole
//! action parses, so a length-truncated completion can never reach the client
//! as a partial answer or partial arguments.

use ferric_iron::terminator::TASK_COMPLETE;
use ferric_iron::{ConstrainedJsonScanner, StreamDelta, parse_json_action};
use serde_json::{Value, json};

/// Why a finished constrained completion cannot be returned as an action.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ActionError {
    #[error("constrained completion did not parse as an action: {0}")]
    Unparsable(String),
    #[error("constrained completion named {0:?}, which was not offered")]
    ToolNotOffered(String),
}

impl ActionError {
    /// Stable receipt label.
    pub fn class(&self) -> &'static str {
        match self {
            ActionError::Unparsable(_) => "unparsable_action",
            ActionError::ToolNotOffered(_) => "tool_not_offered",
        }
    }
}

/// The resolved end of a response.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolution {
    /// The final assistant message (non-streaming body, and what the streamed
    /// deltas assemble to).
    pub message: Value,
    pub finish_reason: String,
    /// Deltas still to stream before the finish chunk.
    pub closing_deltas: Vec<Value>,
    /// The chosen offered tool, or `task_complete`, or `None` when truncated.
    pub tool: Option<String>,
    pub action_valid: bool,
}

/// Accumulates one constrained completion.
#[derive(Debug)]
pub struct ActionAssembler {
    admitted: Vec<String>,
    call_id: String,
    raw: String,
    scanner: ConstrainedJsonScanner,
    named_tool: Option<String>,
    reasoning_streamed: String,
    thought_streamed: String,
}

impl ActionAssembler {
    pub fn new(admitted: Vec<String>, call_id: String) -> Self {
        Self {
            admitted,
            call_id,
            raw: String::new(),
            scanner: ConstrainedJsonScanner::new(),
            named_tool: None,
            reasoning_streamed: String::new(),
            thought_streamed: String::new(),
        }
    }

    /// The raw constrained text received so far.
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Native reasoning the backend streamed outside the constrained content
    /// (a thinking model). It is passed through as `reasoning_content`.
    pub fn push_reasoning(&mut self, text: &str) -> Vec<Value> {
        if text.is_empty() {
            return Vec::new();
        }
        self.reasoning_streamed.push_str(text);
        vec![json!({ "reasoning_content": text })]
    }

    /// Feed constrained content; return the deltas to stream now.
    pub fn push_content(&mut self, text: &str) -> Vec<Value> {
        self.raw.push_str(text);
        let mut deltas = Vec::new();
        for signal in self.scanner.scan(&self.raw) {
            match signal {
                StreamDelta::Thought(thought) if !thought.is_empty() => {
                    self.reasoning_streamed.push_str(&thought);
                    self.thought_streamed.push_str(&thought);
                    deltas.push(json!({ "reasoning_content": thought }));
                }
                StreamDelta::ToolNamed(name)
                    if name != TASK_COMPLETE
                        && self.named_tool.is_none()
                        && self.admitted.contains(&name) =>
                {
                    deltas.push(json!({ "tool_calls": [{
                        "index": 0,
                        "id": self.call_id,
                        "type": "function",
                        "function": { "name": name, "arguments": "" }
                    }]}));
                    self.named_tool = Some(name);
                }
                // The final answer is released only after the whole action
                // parses; see the module docs.
                _ => {}
            }
        }
        deltas
    }

    /// Resolve the completion given the upstream finish reason.
    pub fn finish(&mut self, finish_reason: Option<&str>) -> Result<Resolution, ActionError> {
        if finish_reason == Some("length") {
            return Ok(Resolution {
                message: self.message_base(),
                finish_reason: "length".to_string(),
                closing_deltas: Vec::new(),
                tool: None,
                action_valid: false,
            });
        }
        let call = parse_json_action(0, &self.raw)
            .map_err(|error| ActionError::Unparsable(error.to_string()))?;
        let thought = serde_json::from_str::<Value>(self.raw.trim())
            .ok()
            .and_then(|value| {
                value
                    .get("thought")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_default();

        let mut closing_deltas = Vec::new();
        // Stream any thought the scanner had not yet emitted, so the streamed
        // reasoning always assembles to the final one.
        if let Some(rest) = thought.strip_prefix(self.thought_streamed.as_str())
            && !rest.is_empty()
        {
            self.reasoning_streamed.push_str(rest);
            self.thought_streamed.push_str(rest);
            closing_deltas.push(json!({ "reasoning_content": rest }));
        }

        if call.name == TASK_COMPLETE {
            let summary = match &call.args {
                Value::String(text) => text.clone(),
                other => other
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            };
            closing_deltas.push(json!({ "content": summary }));
            let mut message = self.message_base();
            message["content"] = Value::String(summary);
            return Ok(Resolution {
                message,
                finish_reason: "stop".to_string(),
                closing_deltas,
                tool: Some(TASK_COMPLETE.to_string()),
                action_valid: true,
            });
        }

        if !self.admitted.contains(&call.name) {
            return Err(ActionError::ToolNotOffered(call.name));
        }
        let arguments = serde_json::to_string(&call.args).unwrap_or_else(|_| "{}".to_string());
        if self.named_tool.is_none() {
            closing_deltas.push(json!({ "tool_calls": [{
                "index": 0,
                "id": self.call_id,
                "type": "function",
                "function": { "name": call.name, "arguments": arguments }
            }]}));
        } else {
            closing_deltas.push(json!({ "tool_calls": [{
                "index": 0,
                "function": { "arguments": arguments }
            }]}));
        }
        let mut message = self.message_base();
        message["tool_calls"] = json!([{
            "id": self.call_id,
            "type": "function",
            "function": { "name": call.name, "arguments": arguments }
        }]);
        Ok(Resolution {
            message,
            finish_reason: "tool_calls".to_string(),
            closing_deltas,
            tool: Some(call.name),
            action_valid: true,
        })
    }

    fn message_base(&self) -> Value {
        let mut message = json!({ "role": "assistant", "content": null });
        if !self.reasoning_streamed.is_empty() {
            message["reasoning_content"] = Value::String(self.reasoning_streamed.clone());
        }
        message
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assembler() -> ActionAssembler {
        ActionAssembler::new(
            vec!["read_file".to_string(), TASK_COMPLETE.to_string()],
            "call_abc".to_string(),
        )
    }

    /// Feed `text` in small pieces, as a stream would arrive.
    fn feed(assembler: &mut ActionAssembler, text: &str) -> Vec<Value> {
        let mut deltas = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        for piece in chars.chunks(3) {
            deltas.extend(assembler.push_content(&piece.iter().collect::<String>()));
        }
        deltas
    }

    #[test]
    fn translate_tool_action() {
        let mut a = assembler();
        let live = feed(
            &mut a,
            r#"{"thought":"read it","tool":"read_file","args":{"path":"notes.txt"}}"#,
        );
        let reasoning: String = live
            .iter()
            .filter_map(|d| d["reasoning_content"].as_str())
            .collect();
        assert_eq!(reasoning, "read it");
        let named: Vec<_> = live
            .iter()
            .filter(|d| d.get("tool_calls").is_some())
            .collect();
        assert_eq!(named.len(), 1, "tool name streams once, early");
        assert_eq!(named[0]["tool_calls"][0]["function"]["name"], "read_file");
        assert_eq!(named[0]["tool_calls"][0]["function"]["arguments"], "");

        let resolution = a.finish(Some("stop")).unwrap();
        assert_eq!(resolution.finish_reason, "tool_calls");
        assert_eq!(resolution.tool.as_deref(), Some("read_file"));
        assert_eq!(
            resolution.message,
            json!({
                "role": "assistant",
                "content": null,
                "reasoning_content": "read it",
                "tool_calls": [{
                    "id": "call_abc",
                    "type": "function",
                    "function": {"name": "read_file", "arguments": "{\"path\":\"notes.txt\"}"}
                }]
            })
        );
        assert_eq!(
            resolution.closing_deltas,
            vec![
                json!({"tool_calls": [{"index": 0, "function": {"arguments": "{\"path\":\"notes.txt\"}"}}]})
            ]
        );
    }

    #[test]
    fn translate_final_answer() {
        let mut a = assembler();
        let live = feed(
            &mut a,
            r#"{"thought":"done","tool":"task_complete","args":{"summary":"It says: buy milk."}}"#,
        );
        assert!(
            live.iter().all(|d| d.get("content").is_none()),
            "the answer is not released before the action parses"
        );
        let resolution = a.finish(None).unwrap();
        assert_eq!(resolution.finish_reason, "stop");
        assert_eq!(resolution.message["content"], "It says: buy milk.");
        assert_eq!(resolution.message["reasoning_content"], "done");
        assert!(resolution.message.get("tool_calls").is_none());
        assert_eq!(
            resolution.closing_deltas.last().unwrap(),
            &json!({"content": "It says: buy milk."})
        );
    }

    #[test]
    fn translate_length_is_not_parsed() {
        let mut a = assembler();
        feed(
            &mut a,
            r#"{"thought":"x","tool":"task_complete","args":{"summary":"partial ans"#,
        );
        let resolution = a.finish(Some("length")).unwrap();
        assert_eq!(resolution.finish_reason, "length");
        assert_eq!(resolution.message["content"], Value::Null);
        assert!(resolution.message.get("tool_calls").is_none());
        assert!(resolution.closing_deltas.is_empty());
        assert!(!resolution.action_valid);
    }

    #[test]
    fn translate_rejects_unparsable() {
        let mut a = assembler();
        feed(&mut a, "sure, here is the file");
        assert!(matches!(
            a.finish(Some("stop")),
            Err(ActionError::Unparsable(_))
        ));
    }

    #[test]
    fn translate_rejects_unoffered_tool() {
        let mut a = assembler();
        let live = feed(&mut a, r#"{"thought":"t","tool":"delete_all","args":{}}"#);
        assert!(live.iter().all(|d| d.get("tool_calls").is_none()));
        assert_eq!(
            a.finish(Some("stop")).unwrap_err(),
            ActionError::ToolNotOffered("delete_all".to_string())
        );
    }

    #[test]
    fn native_reasoning_passes_through() {
        let mut a = assembler();
        assert_eq!(
            a.push_reasoning("hmm "),
            vec![json!({"reasoning_content": "hmm "})]
        );
        feed(
            &mut a,
            r#"{"thought":"","tool":"task_complete","args":{"summary":"ok"}}"#,
        );
        let resolution = a.finish(None).unwrap();
        assert_eq!(resolution.message["reasoning_content"], "hmm ");
        assert_eq!(resolution.message["content"], "ok");
    }
}

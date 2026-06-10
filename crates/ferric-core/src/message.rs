use serde::{Deserialize, Serialize};

/// Who produced a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A tool invocation requested by the model.
///
/// `args` is a raw `serde_json::Value` rather than a typed struct: small
/// models emit string, array, and object payloads for the same tool, and the
/// lineage showed that rejecting any of those shapes loses recoverable calls.
/// Validation against the tool's schema happens at dispatch, not here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: serde_json::Value,
}

/// One conversation message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// For `Role::Tool` messages: the id of the tool call being answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn system(text: impl Into<String>) -> Self {
        Self::text_message(Role::System, text)
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self::text_message(Role::User, text)
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self::text_message(Role::Assistant, text)
    }

    /// A tool-result message answering `tool_call_id`.
    pub fn tool_result(tool_call_id: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            text: Some(output.into()),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
        }
    }

    fn text_message(role: Role, text: impl Into<String>) -> Self {
        Self {
            role,
            text: Some(text.into()),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn message_roundtrip_json() {
        let msg = Message {
            role: Role::Assistant,
            text: Some("reading the file".to_string()),
            tool_calls: vec![ToolCall {
                id: "tc-1".to_string(),
                name: "read_file".to_string(),
                args: json!({"path": "src/main.rs"}),
            }],
            tool_call_id: None,
        };
        let encoded = serde_json::to_string(&msg).unwrap();
        let decoded: Message = serde_json::from_str(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn toolcall_args_polymorphic() {
        for args in [json!("x"), json!([1]), json!({"a": 1})] {
            let raw = format!(
                r#"{{"id":"tc-1","name":"t","args":{}}}"#,
                serde_json::to_string(&args).unwrap()
            );
            let call: ToolCall = serde_json::from_str(&raw).unwrap();
            assert_eq!(call.args, args);
        }
    }
}

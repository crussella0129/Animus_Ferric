//! Action grammar and the constrained-JSON parser (moved verbatim from
//! `ferric-loop`, T-12601).
//!
//! - `action_schema` builds the unified JSON-Schema (ADR-015) sent as a
//!   `Constraint` on the `ConstrainedJson` path, so a constraint-honoring
//!   backend can only emit a well-formed action.
//! - `parse_json_action` parses the constrained `{"tool","args"}` completion.
//!
//! The XML fallback parser for unconstrained backends stays in `ferric-loop`
//! (it needs `regex`, which is outside this crate's dependency boundary).

use crate::types::ToolDescriptor;
use ferric_core::ToolCall;
use serde_json::{Value, json};

/// Build the unified action JSON-Schema (ADR-015): an `anyOf` of one
/// const-discriminated `{tool, args}` branch per offered tool, plus the
/// `task_complete` terminator branch. Sent as `Constraint::JsonSchema` on the
/// `ConstrainedJson` path; a constraint-honoring backend enforces it so the
/// completion can only be a well-formed action.
pub fn action_schema(tools: &[ToolDescriptor]) -> Value {
    let branches: Vec<Value> = tools.iter().map(branch_for).collect();
    json!({ "type": "object", "anyOf": branches })
}

fn branch_for(tool: &ToolDescriptor) -> Value {
    json!({
        "type": "object",
        "properties": {
            "thought": { "type": "string", "description": "CRITICAL: Scratchpad space. You MUST use this to draft pseudocode, consider edge cases, and plan the exact logic BEFORE picking a tool or writing code." },
            "tool": { "const": tool.name },
            "args": tool.input_schema.clone(),
        },
        "required": ["thought", "tool", "args"],
        "additionalProperties": false,
    })
}

/// Parse a constrained completion (`{"tool": "...", "args": {...}}`) into a
/// `ToolCall` (id `g-<turn>-0`). Used by the `ConstrainedJson` protocol: the
/// backend has already guaranteed the shape, so this is total over conforming
/// input and returns a typed error otherwise (defense in depth).
pub fn parse_json_action(turn: u32, text: &str) -> Result<ToolCall, ActionParseError> {
    let value: Value =
        serde_json::from_str(text.trim()).map_err(|e| ActionParseError::NotJson(e.to_string()))?;

    let name = match value.get("tool").and_then(Value::as_str) {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => return Err(ActionParseError::MissingTool),
    };
    let args = match value.get("args") {
        Some(a) if a.is_object() => a.clone(),
        Some(_) => return Err(ActionParseError::ArgsNotAnObject),
        None => return Err(ActionParseError::MissingArgs),
    };

    Ok(ToolCall {
        id: format!("g-{turn}-0"),
        name,
        args,
    })
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionParseError {
    MalformedXml,
    NotJson(String),
    MissingTool,
    MissingArgs,
    ArgsNotAnObject,
}

impl std::fmt::Display for ActionParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionParseError::MalformedXml => {
                write!(f, "action did not match <tool_call> XML format")
            }
            ActionParseError::NotJson(e) => write!(f, "action arguments were not valid JSON: {e}"),
            ActionParseError::MissingTool => write!(f, "action missing tool name"),
            ActionParseError::MissingArgs => write!(f, "action missing arguments JSON"),
            ActionParseError::ArgsNotAnObject => write!(f, "action 'args' was not a JSON object"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str) -> ToolDescriptor {
        ToolDescriptor {
            name: name.to_string(),
            description: "d".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
        }
    }

    #[test]
    fn action_schema_branch_count() {
        // N tools → N+1 branches (each tool + task_complete), each a
        // const-discriminated {thought,tool,args} object with additionalProperties:false.
        let schema = action_schema(&[tool("read_file"), tool("write_file")]);
        let branches = schema["anyOf"].as_array().unwrap();
        assert_eq!(branches.len(), 2);
        for b in branches {
            assert!(b["properties"]["tool"]["const"].is_string());
            assert!(b["properties"]["thought"]["type"].is_string());
            assert_eq!(b["additionalProperties"], json!(false));
            assert_eq!(b["required"], json!(["thought", "tool", "args"]));
        }
    }

    #[test]
    fn action_schema_includes_terminator_when_passed() {
        let schema = action_schema(&[tool("read_file"), crate::terminator::descriptor()]);
        let branches = schema["anyOf"].as_array().unwrap();
        assert!(
            branches
                .iter()
                .any(|b| b["properties"]["tool"]["const"] == json!("task_complete"))
        );
    }

    #[test]
    fn parse_json_action_happy() {
        let call = parse_json_action(
            0,
            r#"{"thought": "...", "tool":"read_file","args":{"path":"foo.txt"}}"#,
        )
        .unwrap();
        assert_eq!(call.name, "read_file");
        assert_eq!(call.args, json!({"path":"foo.txt"}));
        assert_eq!(call.id, "g-0-0");
    }

    #[test]
    fn parse_json_action_with_thought() {
        // The model includes a thought field — it must parse cleanly and not
        // leak into the ToolCall's args.
        let call = parse_json_action(
            0,
            r#"{"thought":"I need to read file x","tool":"read_file","args":{"path":"x"}}"#,
        )
        .unwrap();
        assert_eq!(call.name, "read_file");
        assert_eq!(call.args["path"], json!("x"));
        assert!(call.args.get("thought").is_none());
    }

    #[test]
    fn action_schema_includes_thought() {
        let schema = action_schema(&[tool("read_file")]);
        let branches = schema["anyOf"].as_array().unwrap();
        for b in branches {
            assert_eq!(b["properties"]["thought"]["type"], json!("string"));
            // thought is required so the model is forced to reason
            let req = b["required"].as_array().unwrap();
            assert!(req.contains(&json!("thought")));
        }
    }

    #[test]
    fn parse_json_action_rejects_non_object() {
        assert!(matches!(
            parse_json_action(0, r#"["read_file"]"#),
            Err(ActionParseError::MissingTool)
        ));
    }

    #[test]
    fn parse_json_action_rejects_missing_tool() {
        assert!(matches!(
            parse_json_action(0, r#"{"thought": "...", "args":{}}"#),
            Err(ActionParseError::MissingTool)
        ));
    }

    #[test]
    fn parse_json_action_rejects_missing_args() {
        assert!(matches!(
            parse_json_action(0, r#"{"thought": "...", "tool":"read_file"}"#),
            Err(ActionParseError::MissingArgs)
        ));
    }
}

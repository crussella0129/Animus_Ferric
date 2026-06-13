//! The unified action grammar (ADR-015).
//!
//! In UnifiedGrammar mode the model's ENTIRE completion is one JSON object
//! `{"tool": <name>, "args": {...}}`. `action_schema` builds an
//! llguidance-enforceable JSON Schema covering every offered tool plus the
//! `task_complete` terminator as `anyOf` branches; `parse_action` turns a
//! completion's text back into a `ToolCall`. Because the schema is the
//! sampling mask, malformed actions become impossible — the failure that let
//! s1's 1B *describe* `task_complete` in prose instead of calling it.
//!
//! Construct rules (verified against llguidance 1.7.6):
//! - `anyOf`, never `oneOf` (oneOf is rejected by default).
//! - `const` tool-name discriminator, emitted FIRST (insertion order →
//!   document order → early branch commitment); `preserve_order` makes this
//!   real (ADR-016).
//! - `additionalProperties: false` at both object depths.

use ferric_core::ToolCall;
use ferric_provider::ToolDescriptor;
use serde_json::{Value, json};

use crate::terminator;

/// Build the unified action schema over `tools` (already in the deterministic
/// offered order) plus the `task_complete` terminator branch (always last).
pub fn action_schema(tools: &[ToolDescriptor]) -> Value {
    let mut branches: Vec<Value> = tools.iter().map(branch_for).collect();
    branches.push(branch_for(&terminator::descriptor()));
    json!({
        "x-guidance": { "whitespace_flexible": false },
        "type": "object",
        "anyOf": branches,
    })
}

/// One `anyOf` branch: `{tool: const(name), args: <schema>}`. Insertion order
/// puts `tool` before `args` so the discriminator commits the branch early.
fn branch_for(tool: &ToolDescriptor) -> Value {
    let mut props = serde_json::Map::new();
    props.insert("tool".to_string(), json!({ "const": tool.name }));
    props.insert("args".to_string(), args_schema(&tool.input_schema));
    json!({
        "type": "object",
        "properties": Value::Object(props),
        "required": ["tool", "args"],
        "additionalProperties": false,
    })
}

/// Take a tool's input_schema and enforce `additionalProperties: false` on it
/// (the model must emit exactly the declared args). Non-object schemas pass
/// through unchanged.
fn args_schema(input_schema: &Value) -> Value {
    match input_schema {
        Value::Object(map) => {
            let mut out = map.clone();
            out.insert("additionalProperties".to_string(), Value::Bool(false));
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// Parse a UnifiedGrammar completion's text into a `ToolCall`. A valid
/// `{"tool": string, "args": object}` document yields a synthesized call
/// (id `g-<turn>-0`); anything else — non-JSON, wrong shape, missing fields —
/// is a typed error. Never panics, never returns a "final text" action
/// (grammar mode has no text path by design).
pub fn parse_action(turn: u32, text: &str) -> Result<ToolCall, ActionParseError> {
    let value: Value =
        serde_json::from_str(text.trim()).map_err(|e| ActionParseError::NotJson(e.to_string()))?;
    let obj = value.as_object().ok_or(ActionParseError::NotAnObject)?;
    let name = obj
        .get("tool")
        .and_then(|v| v.as_str())
        .ok_or(ActionParseError::MissingTool)?;
    let args = match obj.get("args") {
        Some(args @ Value::Object(_)) => args.clone(),
        Some(_) => return Err(ActionParseError::ArgsNotAnObject),
        None => return Err(ActionParseError::MissingArgs),
    };
    Ok(ToolCall {
        id: format!("g-{turn}-0"),
        name: name.to_string(),
        args,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionParseError {
    NotJson(String),
    NotAnObject,
    MissingTool,
    MissingArgs,
    ArgsNotAnObject,
}

impl std::fmt::Display for ActionParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionParseError::NotJson(e) => write!(f, "completion was not valid JSON: {e}"),
            ActionParseError::NotAnObject => write!(f, "action was not a JSON object"),
            ActionParseError::MissingTool => write!(f, "action missing string 'tool' field"),
            ActionParseError::MissingArgs => write!(f, "action missing 'args' field"),
            ActionParseError::ArgsNotAnObject => write!(f, "action 'args' was not an object"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptors() -> Vec<ToolDescriptor> {
        vec![
            ToolDescriptor {
                name: "read_file".to_string(),
                description: "read".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } },
                    "required": ["path"]
                }),
            },
            ToolDescriptor {
                name: "write_file".to_string(),
                description: "write".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" }, "content": { "type": "string" } },
                    "required": ["path", "content"]
                }),
            },
        ]
    }

    #[test]
    fn schema_golden() {
        let schema = action_schema(&descriptors());
        let text = serde_json::to_string_pretty(&schema).unwrap();

        // anyOf present; oneOf absent (llguidance 1.7.6 rejects oneOf).
        assert!(text.contains("\"anyOf\""), "schema must use anyOf");
        assert!(!text.contains("oneOf"), "schema must NOT contain oneOf");

        // x-guidance whitespace_flexible:false at the root.
        assert_eq!(schema["x-guidance"]["whitespace_flexible"], json!(false));

        // N tools + the terminator branch.
        let branches = schema["anyOf"].as_array().unwrap();
        assert_eq!(branches.len(), 3, "2 tools + task_complete");

        // The terminator is last.
        assert_eq!(
            branches.last().unwrap()["properties"]["tool"]["const"],
            json!("task_complete")
        );

        // additionalProperties:false at both depths in every branch.
        for b in branches {
            assert_eq!(b["additionalProperties"], json!(false), "outer ap:false");
            assert_eq!(
                b["properties"]["args"]["additionalProperties"],
                json!(false),
                "args ap:false"
            );
        }
    }

    #[test]
    fn tool_precedes_args_in_serialization() {
        // preserve_order regression pin: within each branch, "tool" must be
        // serialized before "args" (early branch commitment).
        let schema = action_schema(&descriptors());
        let first = serde_json::to_string(&schema["anyOf"][0]).unwrap();
        let tool_at = first.find("\"tool\"").expect("tool key present");
        let args_at = first.find("\"args\"").expect("args key present");
        assert!(tool_at < args_at, "tool must precede args: {first}");
    }

    #[test]
    fn branches_in_offered_order_terminator_last() {
        let schema = action_schema(&descriptors());
        let names: Vec<&str> = schema["anyOf"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["properties"]["tool"]["const"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["read_file", "write_file", "task_complete"]);
    }

    #[test]
    fn parse_action_roundtrip() {
        let call = parse_action(
            2,
            r#"{"tool":"write_file","args":{"path":"a.txt","content":"hi"}}"#,
        )
        .unwrap();
        assert_eq!(call.name, "write_file");
        assert_eq!(call.id, "g-2-0");
        assert_eq!(call.args["path"], json!("a.txt"));
    }

    #[test]
    fn parse_action_rejects_garbage() {
        assert!(matches!(
            parse_action(0, "not json at all"),
            Err(ActionParseError::NotJson(_))
        ));
    }

    #[test]
    fn parse_action_rejects_partial_json() {
        assert!(matches!(
            parse_action(0, r#"{"tool":"write_file","args":{"path":"a.txt"#),
            Err(ActionParseError::NotJson(_))
        ));
    }

    #[test]
    fn parse_action_rejects_non_action_object() {
        // Valid JSON object that is not an action → typed error, never FinalText.
        assert!(matches!(
            parse_action(0, r#"{"message":"I think I will read the file"}"#),
            Err(ActionParseError::MissingTool)
        ));
        assert!(matches!(
            parse_action(0, r#"{"tool":"read_file"}"#),
            Err(ActionParseError::MissingArgs)
        ));
        assert!(matches!(
            parse_action(0, r#"{"tool":"read_file","args":"oops"}"#),
            Err(ActionParseError::ArgsNotAnObject)
        ));
        assert!(matches!(
            parse_action(0, "[1,2,3]"),
            Err(ActionParseError::NotAnObject)
        ));
    }
}

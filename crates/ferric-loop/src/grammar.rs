//! The XML Regex parser for the XML tool-calling protocol.
//!
//! Parses `<tool_call><name>...</name><args>...</args></tool_call>` from model completions.

use ferric_core::ToolCall;
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

/// Parse a completion's text into a `ToolCall`. A valid XML structure yields a synthesized call
/// (id `g-<turn>-0`); anything else is a typed error.
pub fn parse_action(turn: u32, text: &str) -> Result<ToolCall, ActionParseError> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r"(?s)<tool_call>\s*<name>\s*(.*?)\s*</name>\s*<args>\s*(.*?)\s*</args>\s*</tool_call>",
        )
        .unwrap()
    });

    if let Some(caps) = re.captures(text) {
        let name = caps.get(1).unwrap().as_str().trim().to_string();
        let args_str = caps.get(2).unwrap().as_str().trim();

        if name.is_empty() {
            return Err(ActionParseError::MissingTool);
        }
        if args_str.is_empty() {
            return Err(ActionParseError::MissingArgs);
        }

        let value: Value =
            serde_json::from_str(args_str).map_err(|e| ActionParseError::NotJson(e.to_string()))?;

        if !value.is_object() {
            return Err(ActionParseError::ArgsNotAnObject);
        }

        Ok(ToolCall {
            id: format!("g-{turn}-0"),
            name,
            args: value,
        })
    } else {
        Err(ActionParseError::MalformedXml)
    }
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
    use serde_json::json;

    #[test]
    fn parse_action_roundtrip() {
        let text = r#"
<thought>
I will write to a file.
</thought>
<tool_call>
  <name>write_file</name>
  <args>{"path":"a.txt","content":"hi"}</args>
</tool_call>
"#;
        let call = parse_action(2, text).unwrap();
        assert_eq!(call.name, "write_file");
        assert_eq!(call.id, "g-2-0");
        assert_eq!(call.args["path"], json!("a.txt"));
    }

    #[test]
    fn parse_action_rejects_garbage() {
        assert!(matches!(
            parse_action(0, "not xml at all"),
            Err(ActionParseError::MalformedXml)
        ));
    }

    #[test]
    fn parse_action_rejects_partial_xml() {
        assert!(matches!(
            parse_action(
                0,
                r#"<tool_call><name>write_file</name><args>{"path":"a.txt""#
            ),
            Err(ActionParseError::MalformedXml)
        ));
    }

    #[test]
    fn parse_action_rejects_non_action_object() {
        assert!(matches!(
            parse_action(0, "<tool_call><name></name><args>{}</args></tool_call>"),
            Err(ActionParseError::MissingTool)
        ));
        assert!(matches!(
            parse_action(
                0,
                "<tool_call><name>read_file</name><args></args></tool_call>"
            ),
            Err(ActionParseError::MissingArgs)
        ));
        assert!(matches!(
            parse_action(
                0,
                "<tool_call><name>read_file</name><args>\"oops\"</args></tool_call>"
            ),
            Err(ActionParseError::ArgsNotAnObject)
        ));
    }
}

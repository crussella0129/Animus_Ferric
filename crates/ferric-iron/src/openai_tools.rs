//! Adapting an OpenAI-format `tools` array — the shape Hermes Agent sends on
//! every chat-completions request — into ordered core descriptors (T-12604,
//! INT-0011 AC-3).
//!
//! The adapter is strict about identity (type, name, uniqueness, reserved
//! control names) because a grammar branch is keyed on the tool name: two
//! branches with one name, or a caller tool shadowing `task_complete`, would
//! make the model's choice ambiguous. It is lenient only where OpenAI itself
//! is: a missing description is empty and missing parameters mean "no
//! arguments". Which JSON-Schema constructs the backend can compile is a
//! separate question, answered from live evidence rather than guessed here.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::terminator::{REQUEST_USER_INPUT, SUBMIT_PLAN, TASK_COMPLETE};
use crate::types::ToolDescriptor;

/// Names the constrained protocol reserves for its control branches. A caller
/// tool may not use them.
pub const RESERVED_CONTROL_NAMES: &[&str] = &[TASK_COMPLETE, REQUEST_USER_INPUT, SUBMIT_PLAN];

/// Why an OpenAI `tools` array cannot become core descriptors. Every variant
/// names the offending entry.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OpenAiToolsError {
    #[error("`tools` must be a JSON array")]
    NotAnArray,
    #[error("tool {index}: type {kind:?} is not \"function\"")]
    NotAFunction { index: usize, kind: String },
    #[error("tool {index}: missing or empty function name")]
    MissingName { index: usize },
    #[error("tool {index} ({name}): the name is offered more than once")]
    DuplicateName { index: usize, name: String },
    #[error("tool {index} ({name}): the name is reserved for a protocol control branch")]
    ReservedName { index: usize, name: String },
    #[error("tool {index} ({name}): `parameters` must be a JSON object schema")]
    ParametersNotObject { index: usize, name: String },
}

/// One descriptor per entry of `[{"type":"function","function":{name,
/// description, parameters}}, …]`, in input order.
pub fn descriptors_from_openai_tools(
    tools: &Value,
) -> Result<Vec<ToolDescriptor>, OpenAiToolsError> {
    let entries = tools.as_array().ok_or(OpenAiToolsError::NotAnArray)?;
    let mut seen = BTreeSet::new();
    let mut descriptors = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let kind = entry
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if kind != "function" {
            return Err(OpenAiToolsError::NotAFunction {
                index,
                kind: kind.to_string(),
            });
        }
        let function = entry.get("function").unwrap_or(&Value::Null);
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if name.is_empty() {
            return Err(OpenAiToolsError::MissingName { index });
        }
        if RESERVED_CONTROL_NAMES.contains(&name) {
            return Err(OpenAiToolsError::ReservedName {
                index,
                name: name.to_string(),
            });
        }
        if !seen.insert(name.to_string()) {
            return Err(OpenAiToolsError::DuplicateName {
                index,
                name: name.to_string(),
            });
        }
        let input_schema = match function.get("parameters") {
            None | Some(Value::Null) => json!({"type": "object", "properties": {}}),
            Some(parameters @ Value::Object(_)) => parameters.clone(),
            Some(_) => {
                return Err(OpenAiToolsError::ParametersNotObject {
                    index,
                    name: name.to_string(),
                });
            }
        };
        descriptors.push(ToolDescriptor {
            name: name.to_string(),
            description: function
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            input_schema,
        });
    }
    Ok(descriptors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::action_schema;
    use crate::terminator::control_descriptors;
    use ferric_core::ActionProtocol;

    fn function(name: &str) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": name,
                "description": format!("{name} description"),
                "parameters": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}
            }
        })
    }

    #[test]
    fn openai_tools_adapt_in_order() {
        let descriptors =
            descriptors_from_openai_tools(&json!([function("read_file"), function("write_file")]))
                .unwrap();
        let names: Vec<_> = descriptors.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["read_file", "write_file"]);
        assert_eq!(descriptors[0].description, "read_file description");
        assert_eq!(descriptors[0].input_schema["required"], json!(["path"]));
    }

    #[test]
    fn openai_tools_default_missing_parameters() {
        let descriptors = descriptors_from_openai_tools(&json!([
            {"type": "function", "function": {"name": "list_tasks"}}
        ]))
        .unwrap();
        assert_eq!(descriptors[0].description, "");
        assert_eq!(
            descriptors[0].input_schema,
            json!({"type": "object", "properties": {}})
        );
    }

    #[test]
    fn openai_tools_reject_non_function() {
        let error = descriptors_from_openai_tools(&json!([
            function("read_file"),
            {"type": "web_search"}
        ]))
        .unwrap_err();
        assert_eq!(
            error,
            OpenAiToolsError::NotAFunction {
                index: 1,
                kind: "web_search".to_string()
            }
        );
    }

    #[test]
    fn openai_tools_reject_duplicate_name() {
        let error =
            descriptors_from_openai_tools(&json!([function("read_file"), function("read_file")]))
                .unwrap_err();
        assert_eq!(
            error,
            OpenAiToolsError::DuplicateName {
                index: 1,
                name: "read_file".to_string()
            }
        );
        assert!(error.to_string().contains("read_file"));
    }

    #[test]
    fn openai_tools_reject_reserved_control_name() {
        for reserved in RESERVED_CONTROL_NAMES {
            let error = descriptors_from_openai_tools(&json!([function(reserved)])).unwrap_err();
            assert_eq!(
                error,
                OpenAiToolsError::ReservedName {
                    index: 0,
                    name: reserved.to_string()
                }
            );
        }
    }

    #[test]
    fn openai_tools_reject_missing_name() {
        for entry in [
            json!({"type": "function", "function": {}}),
            json!({"type": "function", "function": {"name": ""}}),
            json!({"type": "function"}),
        ] {
            assert_eq!(
                descriptors_from_openai_tools(&json!([entry])).unwrap_err(),
                OpenAiToolsError::MissingName { index: 0 }
            );
        }
        assert_eq!(
            descriptors_from_openai_tools(&json!({"not": "an array"})).unwrap_err(),
            OpenAiToolsError::NotAnArray
        );
    }

    #[test]
    fn adapted_tools_schema_has_one_branch_each() {
        let mut descriptors =
            descriptors_from_openai_tools(&json!([function("read_file"), function("write_file")]))
                .unwrap();
        descriptors.extend(control_descriptors(ActionProtocol::ConstrainedJson));
        let schema = action_schema(&descriptors);
        let branch_names: Vec<_> = schema["anyOf"]
            .as_array()
            .unwrap()
            .iter()
            .map(|branch| {
                branch["properties"]["tool"]["const"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            branch_names,
            ["read_file", "write_file", REQUEST_USER_INPUT, TASK_COMPLETE]
        );
    }
}

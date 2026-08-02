//! Grammar-visible completion and clarification controls.
//!
//! They are offered to the model as tools but are NEVER registered tools: the
//! loop intercepts them by name before dispatch. This makes completion and a
//! request for required user input first-class actions instead of prompt
//! conventions.

use ferric_core::{ActionProtocol, UserInputRequest, UserInputValidationError};
use ferric_provider::ToolDescriptor;
use serde_json::json;

pub const TASK_COMPLETE: &str = "task_complete";
pub const SUBMIT_PLAN: &str = "submit_plan";
pub const REQUEST_USER_INPUT: &str = "request_user_input";

/// The descriptor offered every tool turn, exempt from `max_tools`.
pub fn descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: TASK_COMPLETE.to_string(),
        description: "Call this when the task is finished. Args: {\"summary\": string} - \
                      a one-sentence summary of what was done."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string", "description": "One-sentence summary of the completed task" }
            },
            "required": ["summary"]
        }),
    }
}

pub fn plan_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: SUBMIT_PLAN.to_string(),
        description:
            "Call this when your implementation plan is complete. Args: {\"plan\": string} - \
                      the full markdown text of the implementation plan."
                .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "plan": { "type": "string", "description": "The full markdown implementation plan" }
            },
            "required": ["plan"]
        }),
    }
}

/// A grammar-visible control action that pauses the loop for one material
/// clarification. Like the completion controls, it is not registered and is
/// therefore independent of the policy's tool-ring cap.
pub fn request_user_input_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: REQUEST_USER_INPUT.to_string(),
        description: "Call this only when a missing user decision or fact materially changes the \
                      correct next action and cannot be recovered from the workspace. This must be \
                      the only tool call in the turn. Use an empty options array for a free-form \
                      answer."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "minLength": 1,
                    "description": "The concrete decision or missing fact the user must supply"
                },
                "context": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Why the answer is material and unavailable from the workspace"
                },
                "options": {
                    "type": "array",
                    "items": { "type": "string", "minLength": 1 },
                    "description": "Suggested answers; use an empty array for free-form input"
                }
            },
            "required": ["question", "context", "options"],
            "additionalProperties": false
        }),
    }
}

/// The control descriptors for one protocol. Keeping these separate from the
/// registry is what makes both controls available regardless of tool-ring cap.
pub fn control_descriptors(protocol: ActionProtocol) -> Vec<ToolDescriptor> {
    let completion = if protocol == ActionProtocol::Plan {
        plan_descriptor()
    } else {
        descriptor()
    };
    // Keep the terminating action last, matching the pre-clarification order.
    vec![request_user_input_descriptor(), completion]
}

pub fn is_task_complete(name: &str) -> bool {
    name == TASK_COMPLETE
}

pub fn is_submit_plan(name: &str) -> bool {
    name == SUBMIT_PLAN
}

pub fn is_request_user_input(name: &str) -> bool {
    name == REQUEST_USER_INPUT
}

/// Decode and semantically validate a clarification request. Unlike the
/// completion terminators, malformed input must not pause a run: callers can
/// return this error to the model and let it issue a corrected request.
pub fn request_of(args: &serde_json::Value) -> Result<UserInputRequest, UserInputRequestError> {
    let request: UserInputRequest = serde_json::from_value(args.clone())?;
    request.validate()?;
    Ok(request)
}

#[derive(Debug, thiserror::Error)]
pub enum UserInputRequestError {
    #[error("invalid request_user_input arguments: {0}")]
    Shape(#[from] serde_json::Error),
    #[error("invalid request_user_input arguments: {0}")]
    Validation(#[from] UserInputValidationError),
}

/// Extract the summary, tolerating malformed args (missing field, non-object,
/// bare string) — a malformed terminator still terminates (plan edge case);
/// it must never turn into a dispatch-error loop.
pub fn summary_of(args: &serde_json::Value) -> String {
    match args {
        serde_json::Value::String(s) => s.clone(),
        other => other
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    }
}

pub fn plan_of(args: &serde_json::Value) -> String {
    match args {
        serde_json::Value::String(s) => s.clone(),
        other => other
            .get("plan")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_descriptor_is_strict_and_supports_free_form() {
        let descriptor = request_user_input_descriptor();
        let schema = descriptor.input_schema;

        assert_eq!(descriptor.name, REQUEST_USER_INPUT);
        assert_eq!(schema["type"], "object");
        assert_eq!(
            schema["required"],
            json!(["question", "context", "options"])
        );
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["properties"]["question"]["minLength"], 1);
        assert_eq!(schema["properties"]["context"]["minLength"], 1);
        assert_eq!(schema["properties"]["options"]["type"], "array");
        assert_eq!(
            schema["properties"]["options"]["items"],
            json!({"type": "string", "minLength": 1})
        );
        assert!(schema["properties"]["options"].get("minItems").is_none());
    }

    #[test]
    fn request_parser_accepts_valid_and_free_form_arguments() {
        let parsed = request_of(&json!({
            "question": "Which database should this target?",
            "context": "Both adapters exist and no default is documented.",
            "options": ["SQLite", "PostgreSQL"]
        }))
        .unwrap();
        assert_eq!(parsed.options, vec!["SQLite", "PostgreSQL"]);

        let free_form = request_of(&json!({
            "question": "What label should be used?",
            "context": "No label is specified in the repository.",
            "options": []
        }))
        .unwrap();
        assert!(free_form.options.is_empty());
    }

    #[test]
    fn request_parser_rejects_malformed_arguments() {
        for malformed in [
            json!("not an object"),
            json!({"context": "why", "options": []}),
            json!({"question": "what?", "context": "why"}),
            json!({"question": " ", "context": "why", "options": []}),
            json!({"question": "what?", "context": " ", "options": []}),
            json!({"question": "what?", "context": "why", "options": [""]}),
            json!({
                "question": "what?",
                "context": "why",
                "options": [],
                "extra": true
            }),
        ] {
            assert!(request_of(&malformed).is_err(), "accepted {malformed}");
        }
    }

    #[test]
    fn controls_include_clarification_in_every_protocol_and_plan_terminates_with_plan() {
        for protocol in [
            ActionProtocol::NativeTools,
            ActionProtocol::ConstrainedJson,
            ActionProtocol::TextXml,
        ] {
            let names: Vec<_> = control_descriptors(protocol)
                .into_iter()
                .map(|descriptor| descriptor.name)
                .collect();
            assert_eq!(names, vec![REQUEST_USER_INPUT, TASK_COMPLETE]);
        }

        let plan_names: Vec<_> = control_descriptors(ActionProtocol::Plan)
            .into_iter()
            .map(|descriptor| descriptor.name)
            .collect();
        assert_eq!(plan_names, vec![REQUEST_USER_INPUT, SUBMIT_PLAN]);
    }

    #[test]
    fn request_name_detection_is_exact() {
        assert!(is_request_user_input(REQUEST_USER_INPUT));
        assert!(!is_request_user_input("request_user_inputs"));
    }
}

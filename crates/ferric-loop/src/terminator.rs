//! The `task_complete` structured terminator (lineage pattern).
//!
//! It is offered to the model as a tool but is NEVER a registered tool: the
//! loop intercepts it by name before dispatch. This is what makes loop
//! termination a first-class, grammar-visible action instead of a prompt
//! convention — the fix that unwedged small models in the lineage.

use ferric_provider::ToolDescriptor;
use serde_json::json;

pub const TASK_COMPLETE: &str = "task_complete";
pub const SUBMIT_PLAN: &str = "submit_plan";

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

pub fn is_task_complete(name: &str) -> bool {
    name == TASK_COMPLETE
}

pub fn is_submit_plan(name: &str) -> bool {
    name == SUBMIT_PLAN
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

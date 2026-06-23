//! Integration tests: `ConstrainedJson` mode — the harness-owns-decoding thesis.
//!
//! The mock returns the action as a raw JSON object in the assistant's TEXT —
//! exactly what a constraint-honoring backend produces under a JSON-Schema
//! `response_format`. The loop parses it via `parse_json_action`, routes it
//! through the SAME dispatch path as native tool calls, sends a real
//! `Constraint` on every request (tools empty), and emits a truthful
//! `ConstraintApplied` event.

mod common;

use common::*;
use ferric_core::{ActionProtocol, Role};
use ferric_loop::StopReason;
use ferric_provider::Constraint;
use serde_json::json;

#[test]
fn constrained_json_dispatches_tool() {
    let result = run_scripted_protocol(
        vec![
            json_completion(
                json!({"tool": "write_file", "args": {"path": "out.txt", "content": "hi"}}),
            ),
            json_completion(json!({"tool": "task_complete", "args": {"summary": "wrote out.txt"}})),
        ],
        &nano_policy(),
        ActionProtocol::ConstrainedJson,
        |provider| {
            // ADR-010 invalid state is unrepresentable: a constraint AND no tools.
            for req in provider.requests() {
                assert!(req.tools.is_empty(), "constrained requests carry no tools");
                assert!(
                    req.constraint.is_some(),
                    "constrained requests carry a constraint"
                );
            }
            let second = &provider.requests()[1];
            assert!(second.messages.iter().any(|m| {
                m.role == Role::User
                    && m.text
                        .as_deref()
                        .unwrap_or_default()
                        .starts_with("[tool_result for write_file]")
            }));
        },
    );
    assert_eq!(result.outcome.stop, StopReason::TaskComplete);
    assert_eq!(result.outcome.final_text.as_deref(), Some("wrote out.txt"));

    let ks = kinds(&result.records);
    // Honesty (ADR-022): ConstrainedJson really applies a constraint → event.
    assert!(ks.contains(&"constraint_applied"));
    assert!(ks.contains(&"tool_call"));
    assert!(ks.contains(&"tool_result"));
    assert_eq!(session_end_reason(&result.records), "task_complete");
}

#[test]
fn constrained_json_carries_action_schema() {
    // The constraint is the unified action schema: an anyOf over the offered
    // tools + task_complete.
    run_scripted_protocol(
        vec![json_completion(
            json!({"tool": "task_complete", "args": {"summary": "done"}}),
        )],
        &nano_policy(),
        ActionProtocol::ConstrainedJson,
        |provider| {
            let req = &provider.requests()[0];
            let constraint = req.constraint.as_ref().expect("constraint present");
            match constraint {
                Constraint::JsonSchema(schema) => {
                    let branches = schema["anyOf"].as_array().expect("anyOf branches");
                    assert!(
                        branches
                            .iter()
                            .any(|b| b["properties"]["tool"]["const"] == json!("task_complete"))
                    );
                }
                other => panic!("expected JsonSchema constraint, got {other:?}"),
            }
        },
    );
}

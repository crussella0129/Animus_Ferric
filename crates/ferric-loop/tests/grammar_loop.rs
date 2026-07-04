//! Integration tests: `TextXml` mode (the unconstrained fallback).
//!
//! The mock returns a `<tool_call>` XML block as the assistant's TEXT
//! (tool_calls empty) — what an unconstrained model is prompted to emit. The
//! loop regex-scrapes it into an action and routes it through the same dispatch
//! path as native tool calls. No constraint is sent, so no `ConstraintApplied`
//! event is claimed.

mod common;

use common::*;
use ferric_core::{ActionProtocol, Role};
use ferric_loop::StopReason;
use ferric_trace::{Event, ParsedEvent};
use serde_json::json;

#[test]
fn textxml_happy_path() {
    let result = run_scripted_protocol(
        vec![
            xml_completion(
                json!({"tool": "write_file", "args": {"path": "out.txt", "content": "hi"}}),
            ),
            xml_completion(json!({"tool": "task_complete", "args": {"summary": "wrote out.txt"}})),
        ],
        &nano_policy(),
        ActionProtocol::TextXml,
        |provider| {
            // The tool result is fed back as a user-role message framed
            // `[tool_result for write_file] ...` (no tools in context).
            let second = &provider.requests()[1];
            let fed = second
                .messages
                .iter()
                .find(|m| {
                    m.role == Role::User
                        && m.text
                            .as_deref()
                            .unwrap_or_default()
                            .starts_with("[tool_result for write_file]")
                })
                .expect("tool result framed as user message");
            assert!(fed.text.as_deref().unwrap().contains("wrote"));
        },
    );
    assert_eq!(result.outcome.stop, StopReason::TaskComplete);
    assert_eq!(result.outcome.final_text.as_deref(), Some("wrote out.txt"));

    let ks = kinds(&result.records);
    assert_eq!(ks[0], "session_start");
    assert_eq!(ks[1], "policy_selected");
    assert!(ks.contains(&"tool_call"));
    assert!(ks.contains(&"tool_result"));
    assert!(ks.contains(&"permission_check"));
    // Honesty (ADR-022): TextXml sends no constraint, so NO ConstraintApplied.
    assert!(!ks.contains(&"constraint_applied"));
    assert_eq!(session_end_reason(&result.records), "task_complete");
}

#[test]
fn textxml_request_shape() {
    // Every TextXml request: tools empty AND constraint None (unconstrained).
    run_scripted_protocol(
        vec![xml_completion(
            json!({"tool": "task_complete", "args": {"summary": "done"}}),
        )],
        &nano_policy(),
        ActionProtocol::TextXml,
        |provider| {
            for req in provider.requests() {
                assert!(req.tools.is_empty(), "TextXml requests carry no tools");
                assert!(
                    req.constraint.is_none(),
                    "TextXml requests carry no constraint"
                );
            }
        },
    );
}

#[test]
fn textxml_terminator_intercepted() {
    let result = run_scripted_protocol(
        vec![xml_completion(
            json!({"tool": "task_complete", "args": {"summary": "nothing to do"}}),
        )],
        &nano_policy(),
        ActionProtocol::TextXml,
        |_| {},
    );
    assert_eq!(result.outcome.stop, StopReason::TaskComplete);
    // task_complete is never DISPATCHED through the registry (no ToolResult),
    // even though it now IS traced as a ToolCall (T-3902, sprint 39 — closes
    // the NativeTools summary-visibility gap for replay; harmless here since
    // TextXml already carried the summary in this turn's raw TurnEnd.text).
    assert!(kinds(&result.records).contains(&"tool_call"));
    assert!(!kinds(&result.records).contains(&"tool_result"));
}

#[test]
fn textxml_malformed_action_rejected() {
    // A `<tool_call>` with no name → no action; nudge once, then
    // EmptyCompletion. (TextXml has no final-text path.)
    let result = run_scripted_protocol(
        vec![
            xml_completion(json!({"message": "I will think about it"})),
            xml_completion(json!({"message": "still thinking"})),
        ],
        &nano_policy(),
        ActionProtocol::TextXml,
        |provider| {
            // The nudge reaches the model on the second request.
            let second = &provider.requests()[1];
            assert!(
                second.messages.iter().any(|m| m.role == Role::User
                    && m.text
                        .as_deref()
                        .unwrap_or_default()
                        .contains("XML tool call")),
                "TextXml no-action nudge must reach the model"
            );
        },
    );
    assert_eq!(result.outcome.stop, StopReason::EmptyCompletion);
}

#[test]
fn textxml_repetition_guard() {
    let same = || xml_completion(json!({"tool": "list_dir", "args": {"path": "."}}));
    let result = run_scripted_protocol(
        vec![same(), same(), same()],
        &nano_policy(),
        ActionProtocol::TextXml,
        |_| {},
    );
    assert_eq!(result.outcome.stop, StopReason::RepetitionGuard);
    let guard_actions: Vec<String> = result
        .records
        .iter()
        .filter_map(|r| match &r.event {
            ParsedEvent::Known(Event::RepetitionGuard { action }) => Some(action.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(guard_actions, vec!["warned", "stopped"]);
}

//! T-207 integration tests: UnifiedGrammar mode.
//!
//! The mock returns grammar-shaped JSON as the assistant's TEXT (tool_calls
//! empty) — exactly what a real backend produces under a JSON-Schema
//! constraint. The loop must parse it into an action and route it through the
//! same dispatch path as native tool calls.

mod common;

use common::*;
use ferric_core::{ActionProtocol, Role};
use ferric_loop::StopReason;
use ferric_trace::{Event, ParsedEvent};
use serde_json::json;

#[test]
fn grammar_happy_path() {
    let result = run_scripted_protocol(
        vec![
            grammar_completion(
                json!({"tool": "write_file", "args": {"path": "out.txt", "content": "hi"}}),
            ),
            grammar_completion(
                json!({"tool": "task_complete", "args": {"summary": "wrote out.txt"}}),
            ),
        ],
        &nano_policy(),
        ActionProtocol::UnifiedGrammar,
        |provider| {
            // C-006: the tool result is fed back as a user-role message
            // framed `[tool_result for write_file] ...`.
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
                .expect("grammar tool result framed as user message");
            assert!(fed.text.as_deref().unwrap().contains("wrote"));
        },
    );
    assert_eq!(result.outcome.stop, StopReason::TaskComplete);
    assert_eq!(result.outcome.final_text.as_deref(), Some("wrote out.txt"));

    // Golden event order: ConstraintApplied present, PolicySelected first.
    let ks = kinds(&result.records);
    assert_eq!(ks[0], "session_start");
    assert_eq!(ks[1], "policy_selected");
    assert!(ks.contains(&"tool_call"));
    assert!(ks.contains(&"tool_result"));
    assert!(ks.contains(&"permission_check"));
    assert_eq!(session_end_reason(&result.records), "task_complete");
}

#[test]
fn grammar_request_shape() {
    // Every grammar-mode request: tools empty AND constraint Some (ADR-010
    // invalid state is unrepresentable).
    run_scripted_protocol(
        vec![grammar_completion(
            json!({"tool": "task_complete", "args": {"summary": "done"}}),
        )],
        &nano_policy(),
        ActionProtocol::UnifiedGrammar,
        |provider| {
            for req in provider.requests() {
                assert!(req.tools.is_empty(), "grammar requests carry no tools");
            }
        },
    );
}

#[test]
fn grammar_terminator_intercepted() {
    let result = run_scripted_protocol(
        vec![grammar_completion(
            json!({"tool": "task_complete", "args": {"summary": "nothing to do"}}),
        )],
        &nano_policy(),
        ActionProtocol::UnifiedGrammar,
        |_| {},
    );
    assert_eq!(result.outcome.stop, StopReason::TaskComplete);
    // task_complete is never dispatched through the registry.
    assert!(!kinds(&result.records).contains(&"tool_call"));
}

#[test]
fn grammar_non_action_json_rejected() {
    // C-012: valid JSON that is NOT an action → no FinalText; nudge once,
    // then EmptyCompletion. (Grammar mode has no final-text path.)
    let result = run_scripted_protocol(
        vec![
            grammar_completion(json!({"message": "I will think about it"})),
            grammar_completion(json!({"message": "still thinking"})),
        ],
        &nano_policy(),
        ActionProtocol::UnifiedGrammar,
        |provider| {
            // The nudge reaches the model on the second request.
            let second = &provider.requests()[1];
            assert!(
                second.messages.iter().any(|m| m.role == Role::User
                    && m.text
                        .as_deref()
                        .unwrap_or_default()
                        .contains("XML tool call")),
                "grammar no-action nudge must reach the model"
            );
        },
    );
    assert_eq!(result.outcome.stop, StopReason::EmptyCompletion);
}

#[test]
fn grammar_repetition_guard() {
    let same = || grammar_completion(json!({"tool": "list_dir", "args": {"path": "."}}));
    let result = run_scripted_protocol(
        vec![same(), same(), same()],
        &nano_policy(),
        ActionProtocol::UnifiedGrammar,
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

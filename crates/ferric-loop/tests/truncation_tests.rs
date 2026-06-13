//! T-208 integration tests: truncated and malformed grammar actions.

mod common;

use common::*;
use ferric_core::{ActionProtocol, Role};
use ferric_loop::StopReason;
use serde_json::json;

#[test]
fn truncated_once_then_recovers() {
    // A cut-off action, then a valid one: nudge once, then complete.
    let result = run_scripted_protocol(
        vec![
            truncated_completion(),
            grammar_completion(json!({"tool": "task_complete", "args": {"summary": "done"}})),
        ],
        &nano_policy(),
        ActionProtocol::UnifiedGrammar,
        |provider| {
            // The truncation nudge reaches the model on the second request,
            // and the partial JSON is NOT in history (only the nudge).
            let second = &provider.requests()[1];
            assert!(
                second.messages.iter().any(|m| m.role == Role::User
                    && m.text.as_deref().unwrap_or_default().contains("cut off")),
                "truncation nudge must reach the model"
            );
            assert!(
                !second.messages.iter().any(|m| m.role == Role::Assistant
                    && m.text.as_deref().unwrap_or_default().contains("got cut o")),
                "partial truncated JSON must not be in history"
            );
        },
    );
    assert_eq!(result.outcome.stop, StopReason::TaskComplete);
}

#[test]
fn truncated_twice_stops() {
    let result = run_scripted_protocol(
        vec![truncated_completion(), truncated_completion()],
        &nano_policy(),
        ActionProtocol::UnifiedGrammar,
        |_| {},
    );
    assert_eq!(result.outcome.stop, StopReason::TruncatedAction);
    assert_eq!(session_end_reason(&result.records), "truncated_action");
}

#[test]
fn truncation_distinct_from_parse_failure() {
    // A non-truncated unparseable completion uses the empty-completion path,
    // NOT truncated_action — the two failure modes stay distinguishable.
    let result = run_scripted_protocol(
        vec![
            grammar_completion(json!({"not": "an action"})),
            grammar_completion(json!({"still": "not"})),
        ],
        &nano_policy(),
        ActionProtocol::UnifiedGrammar,
        |_| {},
    );
    assert_eq!(result.outcome.stop, StopReason::EmptyCompletion);
    assert_eq!(session_end_reason(&result.records), "empty_completion");
}

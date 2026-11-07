//! T-106 integration tests: the hash-ALL-calls repetition guard.

mod common;

use common::*;
use ferric_core::Role;
use ferric_loop::StopReason;
use serde_json::json;

#[test]
fn repetition_warn_then_stop() {
    let result = run_scripted(
        vec![
            tool_completion(same_calls()),
            tool_completion(same_calls()),
            tool_completion(same_calls()),
        ],
        &nano_policy(),
        |provider| {
            // After the warning (2nd identical turn), the nudge must be
            // visible in the 3rd request's messages.
            let third = &provider.requests()[2];
            assert!(
                third.messages.iter().any(|m| m.role == Role::User
                    && m.text
                        .as_deref()
                        .unwrap_or_default()
                        .contains("task_complete")),
                "nudge message (directing task_complete) must reach the model"
            );
        },
    );
    assert_eq!(result.outcome.stop, StopReason::RepetitionGuard);
    assert_eq!(
        guard_actions(&result.records, "repetition_guard"),
        vec!["warned", "stopped"]
    );
    assert_eq!(session_end_reason(&result.records), "repetition_guard");
}

#[test]
fn repetition_resets_on_change() {
    // identical, identical (warn), DIFFERENT, identical — no stop: the
    // counter reset on the change, and the final turn ends with text.
    let mut different = same_calls();
    different[0].2 = json!({"path": "src"});
    let result = run_scripted(
        vec![
            tool_completion(same_calls()),
            tool_completion(same_calls()),
            tool_completion(different),
            tool_completion(same_calls()),
            text_completion("done"),
        ],
        &nano_policy(),
        |_| {},
    );
    assert_eq!(result.outcome.stop, StopReason::FinalText);
    // Exactly one warning, never a stop.
    assert_eq!(
        guard_actions(&result.records, "repetition_guard"),
        vec!["warned"]
    );
}

#[test]
fn order_change_is_not_a_repeat() {
    let calls = same_calls();
    let mut reversed = same_calls();
    reversed.reverse();
    let result = run_scripted(
        vec![
            tool_completion(calls),
            tool_completion(reversed),
            text_completion("done"),
        ],
        &nano_policy(),
        |_| {},
    );
    assert_eq!(result.outcome.stop, StopReason::FinalText);
    assert!(guard_actions(&result.records, "repetition_guard").is_empty());
}

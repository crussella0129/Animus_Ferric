//! T-2802 integration tests: the repeated-failure guard (ADR-038). Exercises
//! the mode the repetition AND no-progress guards both miss — DIFFERENT tools
//! that ALL error every turn (so neither action-keyed guard accumulates; only
//! the result-keyed failure guard fires).

mod common;

use common::*;
use ferric_core::Role;
use ferric_loop::StopReason;
use ferric_trace::{Event, ParsedEvent};
use serde_json::json;

/// A turn whose single call ERRORS: a real builtin on a missing path. The name
/// and args both vary across turns so the repetition + no-progress guards reset.
fn failing_turn(name: &str, path: &str) -> ferric_provider::Completion {
    tool_completion(vec![("c", name, json!({ "path": path }))])
}

fn failure_actions(records: &[ferric_trace::TraceRecord]) -> Vec<String> {
    records
        .iter()
        .filter_map(|r| match &r.event {
            ParsedEvent::Known(Event::FailureGuard { action }) => Some(action.clone()),
            _ => None,
        })
        .collect()
}

fn other_guards_fired(records: &[ferric_trace::TraceRecord]) -> bool {
    records.iter().any(|r| {
        matches!(
            &r.event,
            ParsedEvent::Known(Event::RepetitionGuard { .. })
                | ParsedEvent::Known(Event::NoProgressGuard { .. })
        )
    })
}

#[test]
fn all_failing_turns_warn_then_stop_with_repeated_failure() {
    // Three turns, DIFFERENT tools, all erroring on missing paths: Proceed,
    // Warn (streak 2), Stop (streak 3). The other two guards stay silent.
    let result = run_scripted(
        vec![
            failing_turn("read_file", "missing0.txt"),
            failing_turn("list_dir", "missing1"),
            failing_turn("read_file", "missing2.txt"),
        ],
        &nano_policy(),
        |provider| {
            // The warn nudge (after the 2nd failure) must reach the model in the
            // 3rd request.
            let third = &provider.requests()[2];
            assert!(
                third.messages.iter().any(|m| m.role == Role::User
                    && m.text
                        .as_deref()
                        .unwrap_or_default()
                        .contains("task_complete")),
                "failure nudge must reach the model before the stop"
            );
        },
    );
    assert_eq!(result.outcome.stop, StopReason::RepeatedFailure);
    assert_eq!(failure_actions(&result.records), vec!["warned", "stopped"]);
    assert_eq!(session_end_reason(&result.records), "repeated_failure");
    assert!(
        !other_guards_fired(&result.records),
        "repetition + no-progress must stay silent — different tools/args is their gap"
    );
}

#[test]
fn a_success_resets_and_avoids_the_stop() {
    // Two failing turns (a Warn), then a SUCCEEDING call resets the streak,
    // then text ends the run — no `stopped`.
    let result = run_scripted(
        vec![
            failing_turn("read_file", "missing0.txt"),
            failing_turn("list_dir", "missing1"),
            failing_turn("list_dir", "."), // succeeds: the workspace root exists
            text_completion("done"),
        ],
        &nano_policy(),
        |_| {},
    );
    assert_eq!(result.outcome.stop, StopReason::FinalText);
    assert_eq!(failure_actions(&result.records), vec!["warned"]);
}

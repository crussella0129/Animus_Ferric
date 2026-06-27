//! T-2702 integration tests: the no-progress guard ("semantic flailing",
//! ADR-031/037). Mirrors `repetition_tests.rs` but exercises the mode the
//! repetition guard MISSES — the same tool with DIFFERENT args every turn.

mod common;

use common::*;
use ferric_core::Role;
use ferric_loop::StopReason;
use ferric_trace::{Event, ParsedEvent};
use serde_json::json;

/// One `make_dir` call with a distinct path per turn — same NAME, different
/// args (so the repetition guard sees a fresh signature each turn and never
/// fires; the no-progress guard tracks the name streak).
fn make_dir_turn(i: usize) -> ferric_provider::Completion {
    tool_completion(vec![(
        "c",
        "make_dir",
        json!({ "path": format!("dir{i}") }),
    )])
}

fn progress_actions(records: &[ferric_trace::TraceRecord]) -> Vec<String> {
    records
        .iter()
        .filter_map(|r| match &r.event {
            ParsedEvent::Known(Event::NoProgressGuard { action }) => Some(action.clone()),
            _ => None,
        })
        .collect()
}

fn repetition_fired(records: &[ferric_trace::TraceRecord]) -> bool {
    records
        .iter()
        .any(|r| matches!(&r.event, ParsedEvent::Known(Event::RepetitionGuard { .. })))
}

#[test]
fn flail_warns_then_stops_with_no_progress() {
    // Six same-tool/different-args turns: Proceed×4, Warn (streak 4), Stop
    // (streak 5). The repetition guard must NOT fire (every signature differs).
    let result = run_scripted(
        (0..6).map(make_dir_turn).collect(),
        &nano_policy(),
        |provider| {
            // The warn nudge (after the 5th turn) must reach the model in the
            // 6th request, naming the repeated tool + task_complete.
            let sixth = &provider.requests()[5];
            assert!(
                sixth.messages.iter().any(|m| m.role == Role::User
                    && m.text
                        .as_deref()
                        .unwrap_or_default()
                        .contains("task_complete")),
                "no-progress nudge must reach the model before the stop"
            );
        },
    );
    assert_eq!(result.outcome.stop, StopReason::NoProgress);
    assert_eq!(progress_actions(&result.records), vec!["warned", "stopped"]);
    assert_eq!(session_end_reason(&result.records), "no_progress");
    assert!(
        !repetition_fired(&result.records),
        "repetition guard must stay silent on different-args turns — this is the gap"
    );
}

#[test]
fn a_tool_change_resets_and_avoids_the_stop() {
    // make_dir builds the streak to a Warn, then a DIFFERENT tool resets it,
    // then text ends the run — no `stopped`, ends FinalText.
    let mut script: Vec<_> = (0..5).map(make_dir_turn).collect(); // seed..Warn
    script.push(tool_completion(vec![(
        "c",
        "list_dir",
        json!({"path": "."}),
    )])); // reset
    script.push(text_completion("done"));
    let result = run_scripted(script, &nano_policy(), |_| {});
    assert_eq!(result.outcome.stop, StopReason::FinalText);
    // Exactly one warn, never a stop.
    assert_eq!(progress_actions(&result.records), vec!["warned"]);
}

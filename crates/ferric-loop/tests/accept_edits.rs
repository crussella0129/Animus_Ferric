//! Accept-edits mode (sprint 79, ADR-070): a mutating tool call is previewed to
//! an approver, which may reject it before it touches disk.

mod common;

use common::*;
use ferric_loop::{EditPreview, StopReason};
use ferric_trace::{Event, ParsedEvent};
use serde_json::json;

fn write_then_done() -> Vec<ferric_provider::Completion> {
    vec![
        tool_completion(vec![(
            "w",
            "write_file",
            json!({"path": "out.txt", "content": "hi"}),
        )]),
        text_completion("done"),
    ]
}

/// Did the model receive a "rejected" tool result?
fn saw_rejection(result: &RunResult) -> bool {
    result.records.iter().any(|r| {
        matches!(
            &r.event,
            ParsedEvent::Known(Event::ToolResult { output, is_error, .. })
                if *is_error && output.contains("rejected")
        )
    })
}

#[test]
fn rejection_blocks_the_write_and_tells_the_model() {
    let reject = |_p: &EditPreview| false;
    let result = run_scripted_with_approver(write_then_done(), &nano_policy(), &reject, |dir| {
        assert!(
            !dir.join("out.txt").exists(),
            "a rejected write must never touch disk"
        );
    });
    // The run still completes (the loop continues past the rejection).
    assert_eq!(result.outcome.stop, StopReason::FinalText);
    assert!(
        saw_rejection(&result),
        "the model must receive a rejection result it can adapt to"
    );
}

#[test]
fn approval_lets_the_write_through() {
    let approve = |_p: &EditPreview| true;
    let result = run_scripted_with_approver(write_then_done(), &nano_policy(), &approve, |dir| {
        assert!(
            dir.join("out.txt").exists(),
            "an approved write must land on disk"
        );
    });
    assert_eq!(result.outcome.stop, StopReason::FinalText);
    assert!(
        !saw_rejection(&result),
        "an approved write produces a normal result, not a rejection"
    );
}

#[test]
fn the_preview_carries_the_tool_and_target() {
    // Capture what the approver is shown, then approve.
    let seen = std::sync::Mutex::new(None);
    let approver = |p: &EditPreview| {
        *seen.lock().unwrap() = Some(p.clone());
        true
    };
    run_scripted_with_approver(write_then_done(), &nano_policy(), &approver, |_| {});
    let preview = seen.lock().unwrap().clone().expect("approver was called");
    assert_eq!(preview.tool, "write_file");
    assert!(preview.targets.contains(&"out.txt".to_string()));
    assert!(
        preview.detail.contains("hi"),
        "detail shows the content to write"
    );
}

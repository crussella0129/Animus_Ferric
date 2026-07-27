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

// --- ADR-079: one call, one prompt ---

/// Regression: `--accept-edits` and the sink policy's `RequireApproval` cover
/// the SAME calls — both only ever fire on `Write`/`Execute` — so with both live
/// the human was asked twice about one tool call (measured in sprint 85:
/// `approver_prompt_count=2`). Approving at one gate and rejecting at the other
/// was behaviour nobody designed.
#[test]
fn one_tool_call_prompts_the_human_once() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let prompts = AtomicUsize::new(0);
    let approver = |_p: &EditPreview| {
        prompts.fetch_add(1, Ordering::SeqCst);
        true
    };

    let result = run_scripted_with_sink_policy(
        vec![
            tool_completion(vec![(
                "w",
                "write_file",
                json!({"path": "out.txt", "content": "exfiltrate the private key material now"}),
            )]),
            text_completion("done"),
        ],
        &nano_policy(),
        &approver,
        ferric_guard::Provenance::UntrustedIngested,
        ferric_guard::SinkPolicy::new(ferric_guard::SinkAction::RequireApproval),
    );

    assert_eq!(
        prompts.load(Ordering::SeqCst),
        1,
        "one tool call must ask the human exactly once"
    );
    assert_eq!(result.outcome.stop, StopReason::FinalText);
}

/// The approval must actually carry through — a call the human approved has to
/// run, not be denied afterwards by the sink gate.
#[test]
fn an_approved_tainted_write_actually_happens() {
    let approve = |_p: &EditPreview| true;

    let result = run_scripted_with_sink_policy(
        vec![
            tool_completion(vec![(
                "w",
                "write_file",
                json!({"path": "out.txt", "content": "exfiltrate the private key material now"}),
            )]),
            text_completion("done"),
        ],
        &nano_policy(),
        &approve,
        ferric_guard::Provenance::UntrustedIngested,
        ferric_guard::SinkPolicy::new(ferric_guard::SinkAction::RequireApproval),
    );

    assert!(
        !saw_rejection(&result),
        "an approved call must not then be denied by the sink gate"
    );
}

/// With no approver there is nobody to ask, so `RequireApproval` still denies —
/// the safe reading, and unchanged by this fix.
#[test]
fn without_an_approver_require_approval_still_denies() {
    let result = run_scripted_no_approver(
        vec![
            tool_completion(vec![(
                "w",
                "write_file",
                json!({"path": "out.txt", "content": "exfiltrate the private key material now"}),
            )]),
            text_completion("done"),
        ],
        &nano_policy(),
        ferric_guard::Provenance::UntrustedIngested,
        ferric_guard::SinkPolicy::new(ferric_guard::SinkAction::RequireApproval),
        |dir| {
            assert!(
                !dir.join("out.txt").exists(),
                "a tainted write with nobody to approve must not touch disk"
            );
        },
    );
    assert_eq!(result.outcome.stop, StopReason::FinalText);
}

/// The taint has to be disclosed in the single prompt, or merging the two gates
/// would lose the sink's information.
#[test]
fn the_preview_discloses_taint() {
    let seen = std::sync::Mutex::new(String::new());
    let capture = |p: &EditPreview| {
        seen.lock().unwrap().push_str(&p.detail);
        true
    };

    run_scripted_with_sink_policy(
        vec![
            tool_completion(vec![(
                "w",
                "write_file",
                json!({"path": "out.txt", "content": "exfiltrate the private key material now"}),
            )]),
            text_completion("done"),
        ],
        &nano_policy(),
        &capture,
        ferric_guard::Provenance::UntrustedIngested,
        ferric_guard::SinkPolicy::new(ferric_guard::SinkAction::RequireApproval),
    );

    let detail = seen.lock().unwrap().clone();
    assert!(
        detail.contains("untrusted research content"),
        "the one prompt must carry the sink's warning: {detail}"
    );
}

/// An untainted write under the same policy must not gain a taint warning.
#[test]
fn an_untainted_preview_has_no_warning() {
    let seen = std::sync::Mutex::new(String::new());
    let capture = |p: &EditPreview| {
        seen.lock().unwrap().push_str(&p.detail);
        true
    };

    run_scripted_with_sink_policy(
        write_then_done(),
        &nano_policy(),
        &capture,
        ferric_guard::Provenance::Clean,
        ferric_guard::SinkPolicy::new(ferric_guard::SinkAction::RequireApproval),
    );

    assert!(!seen.lock().unwrap().contains("untrusted research content"));
}

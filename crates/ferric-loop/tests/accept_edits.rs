//! Accept-edits mode (sprint 79, ADR-070): a mutating tool call is previewed to
//! an approver, which may reject it before it touches disk. The interplay with
//! the sink policy's `RequireApproval` gate (ADR-079: one call, one prompt) is
//! pinned in `provenance_gate.rs`.

mod common;

use common::*;
use ferric_loop::{EditPreview, StopReason};

#[test]
fn rejection_blocks_the_write_and_tells_the_model() {
    let reject = |_p: &EditPreview| false;
    let opts = Opts {
        approver: Some(&reject),
        ..Opts::default()
    };
    let result = run_scripted_with(
        write_then_done("hi"),
        &nano_policy(),
        opts,
        |_| {},
        |dir| {
            assert!(
                !dir.join("out.txt").exists(),
                "a rejected write must never touch disk"
            );
        },
    );
    // The run still completes (the loop continues past the rejection).
    assert_eq!(result.outcome.stop, StopReason::FinalText);
    assert!(
        saw_error_result(&result.records, "rejected"),
        "the model must receive a rejection result it can adapt to"
    );
}

#[test]
fn approval_lets_the_write_through() {
    let approve = |_p: &EditPreview| true;
    let opts = Opts {
        approver: Some(&approve),
        ..Opts::default()
    };
    let result = run_scripted_with(
        write_then_done("hi"),
        &nano_policy(),
        opts,
        |_| {},
        |dir| {
            assert!(
                dir.join("out.txt").exists(),
                "an approved write must land on disk"
            );
        },
    );
    assert_eq!(result.outcome.stop, StopReason::FinalText);
    assert!(
        !saw_error_result(&result.records, "rejected"),
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
    let opts = Opts {
        approver: Some(&approver),
        ..Opts::default()
    };
    run_scripted_with(write_then_done("hi"), &nano_policy(), opts, |_| {}, |_| {});
    let preview = seen.lock().unwrap().clone().expect("approver was called");
    assert_eq!(preview.tool, "write_file");
    assert!(preview.targets.contains(&"out.txt".to_string()));
    assert!(
        preview.detail.contains("hi"),
        "detail shows the content to write"
    );
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

    run_scripted_with(
        write_then_done("exfiltrate the private key material now"),
        &nano_policy(),
        tainted(Some(&capture)),
        |_| {},
        |_| {},
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
    let opts = Opts {
        provenance: ferric_guard::Provenance::Clean,
        ..tainted(Some(&capture))
    };

    run_scripted_with(write_then_done("hi"), &nano_policy(), opts, |_| {}, |_| {});

    assert!(!seen.lock().unwrap().contains("untrusted research content"));
}

//! Sprint 72 (ADR-063): prove the harness-internal `tracing` diagnostics
//! actually fire — and stay quiet on a clean run — by capturing them through a
//! scoped buffer-writer subscriber. This is the regression guard that keeps the
//! observability channel honest as the loop evolves.

mod common;

use std::io::Write;
use std::sync::{Arc, Mutex};

use common::*;
use ferric_loop::StopReason;
use serde_json::json;
use tracing_subscriber::fmt::MakeWriter;

/// A `MakeWriter` that appends every formatted line into a shared buffer the
/// test can read back and assert against.
#[derive(Clone)]
struct BufferWriter(Arc<Mutex<Vec<u8>>>);

struct BufferGuard(Arc<Mutex<Vec<u8>>>);

impl Write for BufferGuard {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for BufferWriter {
    type Writer = BufferGuard;
    fn make_writer(&'a self) -> Self::Writer {
        BufferGuard(self.0.clone())
    }
}

/// Run `body` with a WARN-level buffer subscriber installed on this thread, and
/// return everything it logged. `run_scripted` drives the loop synchronously on
/// the current thread, so the thread-local default subscriber captures it.
fn capture_warns(body: impl FnOnce()) -> String {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::fmt()
        .with_writer(BufferWriter(buf.clone()))
        .with_max_level(tracing::Level::WARN)
        .with_ansi(false)
        .finish();
    tracing::subscriber::with_default(subscriber, body);
    String::from_utf8(buf.lock().unwrap().clone()).unwrap()
}

fn same_calls() -> Vec<(&'static str, &'static str, serde_json::Value)> {
    vec![
        ("a", "list_dir", json!({"path": "."})),
        ("b", "read_file", json!({"path": "x.txt"})),
    ]
}

#[test]
fn guard_trip_emits_a_warn() {
    let logged = capture_warns(|| {
        let result = run_scripted(
            vec![
                tool_completion(same_calls()),
                tool_completion(same_calls()),
                tool_completion(same_calls()),
            ],
            &nano_policy(),
            |_| {},
        );
        assert_eq!(result.outcome.stop, StopReason::RepetitionGuard);
    });

    assert!(
        logged.contains("identical action repeated"),
        "the repetition-guard stop must emit its diagnostic WARN; captured: {logged:?}"
    );
    assert!(
        logged.contains("ferric_loop"),
        "the WARN target must identify the loop crate; captured: {logged:?}"
    );
}

#[test]
fn clean_run_stays_quiet_at_warn() {
    // A run that completes with text trips no guard and hits no error path, so
    // nothing should reach the WARN sink — the quiet-by-default guarantee.
    let logged = capture_warns(|| {
        let result = run_scripted(vec![text_completion("done")], &nano_policy(), |_| {});
        assert_eq!(result.outcome.stop, StopReason::FinalText);
    });

    assert!(
        logged.trim().is_empty(),
        "a clean run must emit no WARN-level diagnostics; captured: {logged:?}"
    );
}

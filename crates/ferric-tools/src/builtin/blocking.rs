//! Bridging a synchronous `Tool::run` onto the ambient tokio runtime, without
//! panicking (ADR-074).
//!
//! `Tool::run` is sync, but `shell_exec` and `manage_task` drive
//! `tokio::process`. The obvious bridge —
//! `block_in_place(|| Handle::current().block_on(fut))` — has **two** panic
//! paths, and both are reachable from a model-invoked tool call at the loop's
//! dispatch chokepoint, where a panic kills the whole harness:
//!
//! * `Handle::current()` panics when no runtime is running on this thread, and
//!   `ferric-loop` is explicitly executor-agnostic (its tests drive the loop on
//!   `futures_executor`).
//! * `block_in_place` panics on a **current-thread** runtime, because blocking
//!   the only worker would deadlock it.
//!
//! Both are conditions we can detect, so they become ordinary tool errors the
//! model can read and react to.

use std::future::Future;

/// Run `fut` to completion from synchronous code, or explain why we can't.
///
/// `what` names the operation for the error message — the model sees this.
pub fn block_on_ambient<F: Future>(what: &str, fut: F) -> Result<F::Output, String> {
    let handle = tokio::runtime::Handle::try_current()
        .map_err(|_| format!("{what} needs a tokio runtime, and none is running on this thread"))?;

    if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::CurrentThread {
        return Err(format!(
            "{what} needs a multi-thread tokio runtime (blocking a current-thread \
             runtime would deadlock it)"
        ));
    }

    Ok(tokio::task::block_in_place(|| handle.block_on(fut)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_runtime_is_an_error_not_a_panic() {
        let out = block_on_ambient("probe", async { 1 });
        let err = out.expect_err("no ambient runtime here");
        assert!(err.contains("none is running"), "got: {err}");
    }

    #[tokio::test]
    async fn current_thread_runtime_is_an_error_not_a_panic() {
        // `#[tokio::test]` with no flavor IS a current-thread runtime — the
        // exact shape that used to panic inside `block_in_place`.
        let err = block_on_ambient("probe", async { 1 }).expect_err("current-thread");
        assert!(err.contains("multi-thread"), "got: {err}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn multi_thread_runtime_runs_the_future() {
        assert_eq!(block_on_ambient("probe", async { 41 + 1 }).unwrap(), 42);
    }
}

//! The L0–L6 capability ladder (ADR-019).
//!
//! Spec model (T-211), runner (T-212), verification + results (T-213), and
//! calibration (T-214). Drives the `ferric` binary as a subprocess against
//! TOML level specs and derives metrics from the JSONL trace.

pub mod runner;
pub mod spec;

pub use runner::{Invocation, ModelArgs, RunRecord, WorkspaceHandle, run_spec};
pub use spec::{BenchSpec, ExpectKind, Expectation, embedded_specs};

#[cfg(test)]
mod tests {
    /// ADR-016: preserve_order must be active workspace-wide — the action
    /// grammar depends on insertion-order serialization.
    #[test]
    fn preserve_order_active() {
        let mut obj = serde_json::Map::new();
        obj.insert("zzz".to_string(), serde_json::Value::Null);
        obj.insert("aaa".to_string(), serde_json::Value::Null);
        let text = serde_json::to_string(&obj).unwrap();
        assert!(
            text.find("zzz").unwrap() < text.find("aaa").unwrap(),
            "serde_json must serialize in insertion order (preserve_order)"
        );
    }
}

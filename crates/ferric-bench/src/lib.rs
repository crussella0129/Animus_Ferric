//! The L0–L6 capability ladder (ADR-019).
//!
//! Spec model (T-211), runner (T-212), verification + results (T-213), and
//! calibration (T-214). Drives the `ferric` binary as a subprocess against
//! TOML level specs and derives metrics from the JSONL trace.

mod autonomy;
mod autonomy_results;
mod budget;
mod budget_trace;
mod calibrate;
mod provenance;
mod repository_brief;
mod results;
mod runner;
mod spec;
mod summary;
mod verify;

pub use autonomy::{
    AutonomyCategory, AutonomyTask, EMBEDDED_AUTONOMY_V1, PolicyVariant, RecoveryInjection,
    RecoveryInjectionKind, ResumeRefusalMode, TerminalOutcome, autonomy_bench_spec,
    embedded_autonomy_suite,
};
pub use autonomy_results::{
    AUTONOMY_RESULTS_SCHEMA_VERSION, AutonomyArm, AutonomyEvaluationCoordinate,
    AutonomyEvaluationProvenance, AutonomyResultRow, AutonomyRunIssue, AutonomySegmentResult,
    AutonomyTraceMetrics, ManagedServerProvenance, MechanismCounts, ResumeProbeResult,
    RetainedTraceValidation, STRICT_EVIDENCE_CONTEXT_SIZE, append_autonomy_row,
    executable_is_llama_server, summarize_autonomy_run_with_coordinates, write_autonomy_summary,
};
pub use budget::{
    AttemptIdentity, BudgetControls, ResolvedAgentBudget, RunBudgetEvidence, TraceEvidenceState,
};
pub use budget_trace::retain_budget_trace;
pub use calibrate::{
    ModelProfileRecord, calibrate_from_evidence, read_profile, write_calibrated_ring, write_profile,
};
pub use provenance::{
    git_commit, new_run_id, now_unix_ms, parse_sha256, sha256_bytes, sha256_file,
    system_time_unix_ms,
};
pub use repository_brief::generate_repository_brief;
pub use results::{ResultRow, append_row};
pub use runner::{
    Invocation, OpenAiArgs, QuerySegmentRecord, QuerySegmentRequest, STDERR_TAIL_BYTES,
    materialize_setup_files, run_query_segment, run_spec_with_budget,
};
pub use spec::{BenchSpec, CommandCheck, embedded_specs};
pub use summary::{
    BinaryProvenance, CalibrationEvidence, ModelProvenance, RunIssue, RunProvenance, RunSummary,
    longest_prefix, summarize_run, write_summary,
};
pub use verify::{
    completed, failure_admission, parse_trace, preflight_command_checks, verify_command_checks,
    verify_command_checks_with_deadline, verify_expectations, verify_tools,
};

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

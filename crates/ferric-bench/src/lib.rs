//! The L0–L6 capability ladder (ADR-019).
//!
//! Spec model (T-211), runner (T-212), verification + results (T-213), and
//! calibration (T-214). Drives the `ferric` binary as a subprocess against
//! TOML level specs and derives metrics from the JSONL trace.

pub mod autonomy;
pub mod autonomy_results;
pub mod calibrate;
mod process;
pub mod provenance;
pub mod repository_brief;
pub mod results;
pub mod runner;
pub mod spec;
pub mod summary;
pub mod verify;

pub use autonomy::{
    AUTONOMY_SCHEMA_VERSION, AUTONOMY_TASK_COUNT, AutonomyCategory, AutonomyCheck, AutonomySuite,
    AutonomySuiteError, AutonomyTask, BenchmarkScope, ClarificationContract, CrashPoint,
    EMBEDDED_AUTONOMY_V1, PolicyVariant, RecoveryContract, RecoveryInjection,
    RecoveryInjectionKind, ResumeRefusalMode, TerminalExpectation, TerminalOutcome,
    autonomy_bench_spec, embedded_autonomy_suite, parse_autonomy_suite, validate_autonomy_suite,
};
pub use autonomy_results::{
    AUTONOMY_RESULTS_SCHEMA_VERSION, AutonomyArm, AutonomyEvaluationCoordinate,
    AutonomyEvaluationProvenance, AutonomyRateSummary, AutonomyResultRow, AutonomyRunIssue,
    AutonomyRunSummary, AutonomySegmentResult, AutonomyToolSummary, AutonomyTraceMetrics,
    ClarificationSummary, CoordinateProvenanceSummary, ManagedServerProvenance,
    PairedObjectiveSummary, PassPowerSummary, PolicyMechanismSummary, PolicyPassPowerSummary,
    PolicyRepositoryBriefComparison, RecoverySummary, RepositoryBriefComparison, ResumeProbeResult,
    RetainedTraceValidation, append_autonomy_row, read_autonomy_rows, summarize_autonomy_run,
    summarize_autonomy_run_with_coordinates, write_autonomy_summary,
};
pub use calibrate::{
    ModelProfileRecord, calibrate, calibrate_from_evidence, highest_completed_level,
    longest_completed_prefix, non_monotonic_failures, read_profile, write_calibrated_ring,
    write_profile,
};
pub use provenance::{sha256_bytes, sha256_file};

pub use repository_brief::{RepositoryBrief, RepositoryBriefLimits, generate_repository_brief};
pub use results::{ResultRow, append_row, read_rows};
pub use runner::{
    Invocation, OpenAiArgs, QuerySegmentRecord, QuerySegmentRequest, RunRecord, WorkspaceHandle,
    run_query_segment, run_spec,
};
pub use spec::{BenchSpec, CommandCheck, ExpectKind, Expectation, embedded_specs};
pub use summary::{
    BinaryProvenance, CalibrationEvidence, LevelSummary, ModelProvenance, RunIssue, RunProvenance,
    RunSummary, SampleStats, Wilson95, required_passes, summarize_run, write_summary,
};
pub use verify::{
    CommandCheckResult, CommandCheckStatus, ToolVerdict, TraceMetrics, completed,
    failure_admission, parse_trace, preflight_command_checks, verify_command_checks,
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

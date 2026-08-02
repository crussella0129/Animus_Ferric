//! Tool trait, registry chokepoint, and builtin file tools for Animus Ferric.
//!
//! Both legacy execution and evidence-controlled prepare/commit flow through
//! registry chokepoints that enforce guards, timing, and the
//! full-vs-model-facing output split.

pub mod builtin;
mod control;
mod registry;
mod spec;

pub use builtin::{
    NamedCheck, RunCheck, register_builtin_tools, register_human_tools, register_run_checks,
};
pub use control::{
    ControlFailure, ControlFailureKind, ControlMetadata, FileObservation, LineRange,
    MutationIntent, MutationKind, NavigationKind, NavigationObservation, ObservationRequirement,
    PathState, PrepareCtx, PrepareError, PrepareErrorKind, PreparedIntent, RequestedLineRange,
    ToolObservation, ToolPreparation, VerificationAttempt, VerificationIntent, WorkspaceEffect,
    WorkspaceEffectReport,
};
pub use registry::{
    ApprovalRequest, CheckRecord, ControlledOutcome, DEFAULT_TRUNCATION_LIMIT, ExecuteOutcome,
    PrepareOutcome, PreparedCall, Registry, SinkApprover, ToolOutput, truncate_for_model,
};
pub use spec::{Tool, ToolCtx, ToolSpec};

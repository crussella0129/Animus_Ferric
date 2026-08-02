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
    CandidatePathState, ControlCapability, ControlFailure, ControlFailureKind,
    ControlFailureWitness, ControlMetadata, FileObservation, LineRange, MutationIntent,
    MutationKind, NavigationKind, NavigationObservation, NoEffectKind, ObservationRequirement,
    PathState, PrepareCtx, PrepareError, PrepareErrorKind, PrepareFailureWitness, PreparedIntent,
    RequestedLineRange, StaleObservationWitness, SyntaxState, SyntaxTransition,
    SyntaxUncheckedReason, ToolObservation, ToolPreparation, UnsupportedMutationKind,
    VerificationAttempt, VerificationIntent, WorkspaceEffect, WorkspaceEffectKind,
    WorkspaceEffectReport, sha256_bytes,
};
pub use registry::{
    ApprovalRequest, CheckRecord, ControlledOutcome, DEFAULT_TRUNCATION_LIMIT, ExecuteOutcome,
    PrepareOutcome, PreparedCall, Registry, SinkApprover, ToolOutput, truncate_for_model,
};
pub use spec::{Tool, ToolCtx, ToolSpec};

//! Tool trait, registry chokepoint, and builtin file tools for Animus Ferric.
//!
//! Every tool execution flows through a single registry chokepoint that
//! performs the guard check, timing, and the full-vs-truncated output split.

pub mod builtin;
mod registry;
mod spec;

pub use builtin::{
    NamedCheck, RunCheck, register_builtin_tools, register_human_tools, register_run_checks,
};
pub use registry::{
    ApprovalRequest, CheckRecord, DEFAULT_TRUNCATION_LIMIT, ExecuteOutcome, Registry, SinkApprover,
    ToolOutput, truncate_for_model,
};
pub use spec::{Tool, ToolCtx, ToolSpec};

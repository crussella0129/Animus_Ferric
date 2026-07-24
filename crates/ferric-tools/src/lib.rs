//! Tool trait, registry chokepoint, and builtin file tools for Animus Ferric.
//!
//! Every tool execution flows through a single registry chokepoint that
//! performs the guard check, timing, and the full-vs-truncated output split.

pub mod builtin;
mod registry;
mod spec;

pub use builtin::register_builtin_tools;
pub use registry::{
    CheckRecord, DEFAULT_TRUNCATION_LIMIT, ExecuteOutcome, Registry, ToolOutput, truncate_for_model,
};
pub use spec::{Tool, ToolCtx, ToolSpec};

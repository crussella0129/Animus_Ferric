//! Hardcoded security for Animus Ferric: workspace boundary, permissions, deny lists.
//!
//! The LLM is never consulted on a security decision. Everything here is
//! compile-time policy with no runtime mutation API.

pub mod checker;
pub mod denylist;
pub mod sink;
pub mod workspace;

pub use checker::{Decision, DenyReason, PermissionLevel, check};
pub use sink::{SinkAction, SinkDecision, SinkPolicy, TaintSet};
pub use workspace::{GuardError, Workspace};

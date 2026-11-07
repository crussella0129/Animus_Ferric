//! Hardcoded security for Animus Ferric: workspace boundary, permissions, deny lists.
//!
//! The LLM is never consulted on a security decision. Everything here is
//! compile-time policy with no runtime mutation API.

mod checker;
mod denylist;
mod ignore;
mod sink;
mod workspace;

pub use checker::{Decision, DenyReason, PermissionLevel, check, check_command, check_with_ignore};
pub use ignore::IgnoreList;
pub use sink::{Provenance, SinkAction, SinkDecision, SinkPolicy};
pub use workspace::{AdmitError, GuardError, Workspace};

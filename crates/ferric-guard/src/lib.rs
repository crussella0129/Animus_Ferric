//! Hardcoded security for Animus Ferric: workspace boundary, permissions, deny lists.
//!
//! The LLM is never consulted on a security decision. Everything here is
//! compile-time policy with no runtime mutation API.

mod checker;
pub mod denylist;
mod workspace;

pub use checker::{Decision, DenyReason, PermissionLevel, check};
pub use workspace::{GuardError, Workspace};

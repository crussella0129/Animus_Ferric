//! Hardcoded security for Animus Ferric: workspace boundary, permissions, deny lists.
//!
//! The LLM is never consulted on a security decision. Everything here is
//! compile-time policy with no runtime mutation API.

mod workspace;

pub use workspace::{GuardError, Workspace};

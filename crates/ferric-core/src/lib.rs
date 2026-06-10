//! Shared vocabulary types and the deterministic scale function for Animus Ferric.

mod error;
mod message;

pub use error::FerricError;
pub use message::{Message, Role, ToolCall};

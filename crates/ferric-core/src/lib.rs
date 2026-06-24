//! Shared vocabulary types and the deterministic scale function for Animus Ferric.

mod error;
mod media;
mod message;
mod scale;

pub use error::FerricError;
pub use media::{
    Attachment, FileKind, MediaPart, Modality, classify_path, decide_attachment, modality_flag,
    parse_modalities,
};
pub use message::{Message, Role, ToolCall};
pub use scale::{
    ActionProtocol, ModelProfile, Protocol, RunPolicy, Tier, policy_for, tier_for_level,
    tier_for_params,
};

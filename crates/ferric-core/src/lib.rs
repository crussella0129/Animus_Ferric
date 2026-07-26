//! Shared vocabulary types and the deterministic scale function for Animus Ferric.

mod error;
pub mod hooks;
mod media;
mod message;
mod scale;

pub use error::FerricError;
pub use hooks::HooksConfig;
pub use media::{
    Attachment, FileKind, MediaPart, Modality, base64_encode, classify_path, decide_attachment,
    modality_flag, parse_modalities,
};
pub use message::{Message, Role, ToolCall};
pub use scale::{
    ActionProtocol, DEFAULT_TRUNCATION_LIMIT, ModelProfile, RunPolicy, Tier,
    default_truncation_limit, policy_for, protocol_key, ring_for_tier, tier_for_level,
    tier_for_params,
};

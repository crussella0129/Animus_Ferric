//! Shared vocabulary types and the deterministic scale function for Animus Ferric.

mod check_env;
mod error;
mod fit;
mod harness;
mod hooks;
mod message;
mod scale;
mod user_input;

pub use check_env::configure_check_environment;
pub use error::FerricError;
pub use fit::{Fit, classify_fit, estimate_model_memory};
pub use harness::{HarnessPolicy, ParseHarnessPolicyError};
pub use hooks::HooksConfig;
pub use message::{MediaPart, Message, Role, ToolCall};
pub use scale::{
    ActionProtocol, DEFAULT_TRUNCATION_LIMIT, ModelProfile, OutputBudget, OutputBudgetSource,
    RunPolicy, Tier, TierSource, default_truncation_limit, policy_for, policy_for_with_override,
    protocol_key, resolve_output_budget, ring_for_tier, tier_decision, tier_for_level,
    tier_for_params,
};
pub use user_input::{UserInputRequest, UserInputValidationError};

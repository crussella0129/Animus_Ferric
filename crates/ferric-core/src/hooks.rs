use serde::{Deserialize, Serialize};

/// Optional configuration for executing local hooks synchronously during the agent loop.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HooksConfig {
    /// Command string to execute before `TurnStart` logic. If it returns a non-zero exit code,
    /// the turn stops with `HookFailed("pre_turn")`.
    pub pre_turn: Option<String>,
    /// Command string to execute at the end of the turn (after `TurnEnd` and VCS snapshot).
    pub post_turn: Option<String>,
    /// Command string to execute when the agent stops gracefully or fatally with an error
    /// (e.g. `ProviderError`, `HookFailed`).
    pub on_error: Option<String>,
}

impl HooksConfig {
    /// Returns true if all hooks are None.
    pub fn is_empty(&self) -> bool {
        self.pre_turn.is_none() && self.post_turn.is_none() && self.on_error.is_none()
    }
}

//! Hash-ALL-calls repetition guard (Prion failure-mode #5).
//!
//! The lineage's original guard hashed only the first tool call and was
//! evaded by multi-tool loops. This one canonicalizes the COMPLETE ordered
//! set of a turn's calls (names + args; ids excluded — they vary per turn).
//! Two-strike design: first repeat warns and nudges, second consecutive
//! repeat stops the loop.

use ferric_core::ToolCall;

pub struct RepetitionGuard {
    last_signature: Option<String>,
    consecutive_repeats: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Proceed,
    Warn,
    Stop,
}

impl RepetitionGuard {
    pub fn new() -> Self {
        Self {
            last_signature: None,
            consecutive_repeats: 0,
        }
    }

    /// Observe a turn's complete tool-call set and decide.
    pub fn observe(&mut self, calls: &[ToolCall]) -> Verdict {
        let signature = signature_of(calls);
        let repeated = self.last_signature.as_deref() == Some(signature.as_str());
        self.last_signature = Some(signature);
        if !repeated {
            self.consecutive_repeats = 0;
            return Verdict::Proceed;
        }
        self.consecutive_repeats += 1;
        match self.consecutive_repeats {
            1 => Verdict::Warn,
            _ => Verdict::Stop,
        }
    }
}

impl Default for RepetitionGuard {
    fn default() -> Self {
        Self::new()
    }
}

/// Canonical signature: ordered (name, args) pairs. serde_json::Value's
/// Display is deterministic for a given value, which is sufficient here —
/// both sides of the comparison go through the same serializer.
fn signature_of(calls: &[ToolCall]) -> String {
    calls
        .iter()
        .map(|c| format!("{}\u{1f}{}", c.name, c.args))
        .collect::<Vec<_>>()
        .join("\u{1e}")
}

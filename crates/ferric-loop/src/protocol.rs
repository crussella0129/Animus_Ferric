//! Action-protocol selection (ADR-015).

use ferric_core::{ActionProtocol, RunPolicy};
use ferric_provider::Capabilities;

/// Decide how the loop talks to the backend about actions.
///
/// An explicit `override_` always wins. Absent an override the default is
/// `NativeTools`.
///
/// NOTE (ADR-020, s2 finding): UnifiedGrammar is NOT the auto-default even
/// though the policy wants constrained JSON and the backend reports
/// constraint support. The s2 real-GGUF gate found that
/// `mistralrs::send_chat_request` with a `Constraint::JsonSchema` HANGS on the
/// 1B (engine-level llguidance pathology — our model-free grammar tests all
/// pass, so the loop is correct; the hang is downstream). Until that is
/// root-caused, grammar mode is opt-in via `--protocol grammar`; the auto
/// path stays on the proven NativeTools backend so `ferric query` never hangs
/// by default. The `_caps` argument is retained for the eventual re-enable.
pub fn select_protocol(
    policy: &RunPolicy,
    _caps: &Capabilities,
    override_: Option<ActionProtocol>,
) -> ActionProtocol {
    if let Some(p) = override_ {
        return p;
    }
    // Auto-default: NativeTools (see note above). `policy.protocol` is read so
    // the signature stays stable when grammar is re-enabled as the default.
    let _ = policy.protocol;
    ActionProtocol::NativeTools
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_core::{ModelProfile, policy_for};

    fn caps() -> Capabilities {
        Capabilities {
            supports_native_tool_calls: true,
            exposes_logits: false,
        }
    }

    fn nano() -> RunPolicy {
        // NANO policy.protocol is ConstrainedJson.
        policy_for(&ModelProfile {
            params_b: 1.0,
            quant: "Q4_K_M".to_string(),
            ctx: 4096,
            family: "t".to_string(),
            measured_level: None,
        })
    }

    #[test]
    fn auto_default_is_native_pending_grammar_root_cause() {
        // ADR-020: grammar is NOT auto-selected even when capable — it hangs
        // the real engine. Default must be NativeTools so query never hangs.
        assert_eq!(
            select_protocol(&nano(), &caps(), None),
            ActionProtocol::NativeTools
        );
    }

    #[test]
    fn override_to_grammar_wins() {
        // Grammar is opt-in via explicit override (`--protocol grammar`).
        assert_eq!(
            select_protocol(&nano(), &caps(), Some(ActionProtocol::UnifiedGrammar)),
            ActionProtocol::UnifiedGrammar
        );
    }

    #[test]
    fn override_to_native_wins() {
        assert_eq!(
            select_protocol(&nano(), &caps(), Some(ActionProtocol::NativeTools)),
            ActionProtocol::NativeTools
        );
    }
}

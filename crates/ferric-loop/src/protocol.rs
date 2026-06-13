//! Action-protocol selection (ADR-015).

use ferric_core::{ActionProtocol, Protocol, RunPolicy};
use ferric_provider::Capabilities;

/// Decide how the loop talks to the backend about actions.
///
/// An explicit `override_` wins. Otherwise: a policy that wants constrained
/// JSON gets `UnifiedGrammar` *if the backend can enforce a constraint*,
/// falling back to `NativeTools` when it can't. Non-constrained policies use
/// `NativeTools`.
pub fn select_protocol(
    policy: &RunPolicy,
    caps: &Capabilities,
    override_: Option<ActionProtocol>,
) -> ActionProtocol {
    if let Some(p) = override_ {
        return p;
    }
    match policy.protocol {
        Protocol::ConstrainedJson if caps.supports_constraint => ActionProtocol::UnifiedGrammar,
        _ => ActionProtocol::NativeTools,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_core::{ModelProfile, policy_for};

    fn caps(supports_constraint: bool) -> Capabilities {
        Capabilities {
            supports_constraint,
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
    fn constrained_capable_selects_grammar() {
        assert_eq!(
            select_protocol(&nano(), &caps(true), None),
            ActionProtocol::UnifiedGrammar
        );
    }

    #[test]
    fn constrained_incapable_falls_back_to_native() {
        assert_eq!(
            select_protocol(&nano(), &caps(false), None),
            ActionProtocol::NativeTools
        );
    }

    #[test]
    fn override_wins() {
        assert_eq!(
            select_protocol(&nano(), &caps(true), Some(ActionProtocol::NativeTools)),
            ActionProtocol::NativeTools
        );
    }
}

//! Action-protocol selection (ADR-015/022).

use ferric_core::{ActionProtocol, RunPolicy};
use ferric_provider::Capabilities;

/// Decide how the loop talks to the backend about actions, from the backend's
/// real `Capabilities`. An explicit `override_` always wins.
///
/// Preference order (ADR-022): a backend that enforces a harness-authored
/// constraint server-side (`supports_constraint`, e.g. the HTTP valve via
/// `response_format`) runs `ConstrainedJson` — the "harness owns decoding"
/// thesis. Else a backend that speaks native tool calling runs `NativeTools`.
/// Else `TextXml`, the honest fallback: the model is prompted to emit
/// `<tool_call>` XML and the loop regex-scrapes it, claiming no constraint.
///
/// A backend advertising neither capability lands on `TextXml`, the honest
/// fallback — the constrained thesis lives on the backend that can actually
/// enforce it. (This was the in-process mistral.rs path, whose constrained
/// decoding hung upstream — ADR-020/027; it has since been removed, but the
/// fallback stays correct for any such backend.) `_policy` is retained for
/// signature stability (selection is capability-driven, not model-size-driven).
pub fn select_protocol(
    _policy: &RunPolicy,
    caps: &Capabilities,
    override_: Option<ActionProtocol>,
) -> ActionProtocol {
    if let Some(p) = override_ {
        return p;
    }
    if caps.supports_constraint {
        ActionProtocol::ConstrainedJson
    } else if caps.supports_native_tool_calls {
        ActionProtocol::NativeTools
    } else {
        ActionProtocol::TextXml
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_core::{ModelProfile, policy_for};

    fn caps(native: bool, constraint: bool) -> Capabilities {
        Capabilities {
            supports_native_tool_calls: native,
            supports_constraint: constraint,
            exposes_logits: false,
            supports_media: false,
        }
    }

    fn nano() -> RunPolicy {
        policy_for(&ModelProfile {
            params_b: 1.0,
            quant: "Q4_K_M".to_string(),
            ctx: 4096,
            family: "t".to_string(),
            measured_level: None,
        })
    }

    #[test]
    fn constraint_capable_selects_constrained_json() {
        // The thesis: a constraint-honoring backend runs ConstrainedJson.
        assert_eq!(
            select_protocol(&nano(), &caps(true, true), None),
            ActionProtocol::ConstrainedJson
        );
    }

    #[test]
    fn native_only_selects_native_tools() {
        assert_eq!(
            select_protocol(&nano(), &caps(true, false), None),
            ActionProtocol::NativeTools
        );
    }

    #[test]
    fn neither_selects_text_xml() {
        // A backend with neither constraint nor native tool calling → honest XML.
        assert_eq!(
            select_protocol(&nano(), &caps(false, false), None),
            ActionProtocol::TextXml
        );
    }

    #[test]
    fn override_always_wins() {
        assert_eq!(
            select_protocol(&nano(), &caps(true, true), Some(ActionProtocol::TextXml)),
            ActionProtocol::TextXml
        );
        assert_eq!(
            select_protocol(
                &nano(),
                &caps(false, false),
                Some(ActionProtocol::ConstrainedJson)
            ),
            ActionProtocol::ConstrainedJson
        );
        assert_eq!(
            select_protocol(
                &nano(),
                &caps(true, true),
                Some(ActionProtocol::NativeTools)
            ),
            ActionProtocol::NativeTools
        );
    }
}

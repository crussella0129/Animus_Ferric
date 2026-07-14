//! The deterministic scale function: `ModelProfile` → `RunPolicy`.
//!
//! This is Ferric's central design idea: how much agency a run gets (plan
//! granularity, turn budgets, tool count, action protocol) is a *pure
//! function* of what the model is. No filename sniffing, no runtime
//! heuristics, no LLM self-report — profiles are config-supplied (the
//! lineage's H8/H20 tier-misdetection traps are unrepresentable), and a
//! measured capability level from the L0–L6 benchmark ladder overrides the
//! parameter-count prior in BOTH directions.
//!
//! The table values are a calibration *seed* carried over from Animus
//! `tiers.py` and the small-model performance findings; they are pinned by a
//! snapshot test so every change is a reviewed diff. Empirical calibration is
//! a later sprint.

use serde::{Deserialize, Serialize};

/// A description of the model a run will use. Config-supplied, never inferred.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelProfile {
    /// Parameter count in billions.
    pub params_b: f32,
    /// Quantization label, e.g. `"Q4_K_M"`. Informational in s0.
    pub quant: String,
    /// Context window in tokens.
    pub ctx: u32,
    /// Model family, e.g. `"qwen2.5-coder"`. Informational in s0.
    pub family: String,
    /// Measured capability level on the L0–L6 ladder, when benchmarked.
    /// Takes precedence over `params_b` for tier selection.
    pub measured_level: Option<u8>,
}

/// Capability tier. Ordering is meaningful: `Nano < Small < … < Ultra`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Nano,
    Small,
    Medium,
    Large,
    Xl,
    Ultra,
}

/// The action format the model is asked to use.
///
/// `ConstrainedJson` is the default everywhere because the harness owns
/// decoding; `FencedCode` and `EditFormat` are downgrade paths for backends
/// that cannot enforce grammars (capability-driven, selected by the loop —
/// not by model size).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    ConstrainedJson,
    FencedCode,
    EditFormat,
}

/// How the loop talks to the backend about actions (ADR-015/022). Selected
/// from backend `Capabilities` by `select_protocol`:
///
/// - `NativeTools`: tools + tool_choice set, no constraint — the backend's
///   template-native tool format; the loop reads structured `tool_calls`.
/// - `ConstrainedJson`: ONE JSON-Schema `Constraint` over the whole action
///   space, tools empty — a constraint-honoring backend (the HTTP valve)
///   enforces it server-side so malformed actions are unrepresentable. The
///   loop parses the completion as `{tool, args}` JSON. This is the founding
///   "harness owns decoding" thesis. (Formerly `UnifiedGrammar`; serde alias
///   kept so older traces/bench rows still read.)
/// - `TextXml`: no constraint, no tools — the honest fallback for backends
///   that enforce neither; the model is prompted to emit `<tool_call>` XML and
///   the loop regex-scrapes it. No `ConstraintApplied` is claimed.
///
/// `ConstrainedJson`'s constraint and `NativeTools`'s tools are mutually
/// exclusive by construction (ADR-010).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionProtocol {
    NativeTools,
    #[serde(alias = "unified_grammar")]
    ConstrainedJson,
    TextXml,
}

/// Everything the agent loop needs to know about how to run a given model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunPolicy {
    pub tier: Tier,
    pub protocol: Protocol,
    pub uses_planner: bool,
    pub max_plan_steps: u8,
    pub max_turns_per_step: u8,
    pub max_turns: u8,
    pub max_tools: u8,
    pub prompt_budget_tokens: u32,
    /// Per-turn generation cap (ADR-018). Caps worst-case turn wall-time and
    /// leaves headroom over the largest expected single action (~450 tokens).
    pub max_output_tokens: u32,
    pub allows_subagents: bool,
    /// Operator cap on the active tool ring (ADR-028). `None` ⇒ the tier's
    /// `ring_for_tier` ceiling; `Some(n)` caps the active rings at
    /// `min(tier_ceiling, n)` — restrict-only (raise via `measured_level`). The
    /// CLI `--max-ring` sets it; `policy_for` leaves it `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_ring: Option<u8>,
}

/// Tier from parameter count. Boundaries follow Animus `tiers.py`:
/// NANO < 4B ≤ SMALL < 13B ≤ MEDIUM < 30B ≤ LARGE < 70B ≤ XL < 200B ≤ ULTRA.
pub fn tier_for_params(params_b: f32) -> Tier {
    if params_b < 4.0 {
        Tier::Nano
    } else if params_b < 13.0 {
        Tier::Small
    } else if params_b < 30.0 {
        Tier::Medium
    } else if params_b < 70.0 {
        Tier::Large
    } else if params_b < 200.0 {
        Tier::Xl
    } else {
        Tier::Ultra
    }
}

/// Tier from a measured L0–L6 capability level. Seed mapping: a model that
/// breaks at L2 (measured L1) is NANO-grade regardless of size; one that
/// completes multi-file construction (L4) earns SMALL-grade agency; L5/L6
/// earn MEDIUM/LARGE. Levels above 6 are clamped to 6.
pub fn tier_for_level(level: u8) -> Tier {
    match level.min(6) {
        0 | 1 => Tier::Nano,
        2..=4 => Tier::Small,
        5 => Tier::Medium,
        _ => Tier::Large,
    }
}

/// Tool-vocabulary ring ceiling for a tier (the rings model). A run may use
/// rings `0..=ring_for_tier(tier)`. Ring 0 is the always-on navigate/mutate
/// core (every model); higher rings (find/organize, plan/diff, external tools)
/// unlock with capability — and since `tier` honours `measured_level`, a model
/// is promoted to a wider ring set by demonstrated reliability, not size alone.
/// Rings 2–3 are reserved for tools that land in later sprints.
pub fn ring_for_tier(tier: Tier) -> u8 {
    match tier {
        Tier::Nano => 0,
        Tier::Small => 1,
        Tier::Medium => 2,
        Tier::Large | Tier::Xl | Tier::Ultra => 3,
    }
}

/// Per-tier seed row: (uses_planner, max_plan_steps, max_turns_per_step,
/// max_turns, max_tools, prompt_budget_cap, max_output_tokens, allows_subagents).
#[allow(clippy::type_complexity)]
fn tier_row(tier: Tier) -> (bool, u8, u8, u8, u8, u32, u32, bool) {
    match tier {
        Tier::Nano => (true, 3, 5, 15, 10, 2_800, 512, false),
        Tier::Small => (true, 5, 4, 20, 14, 5_600, 768, false),
        Tier::Medium => (false, 1, 25, 25, 20, 11_200, 1_024, false),
        Tier::Large => (false, 1, 40, 40, 28, 22_400, 1_536, true),
        Tier::Xl => (false, 1, 60, 60, 36, 44_800, 2_048, true),
        Tier::Ultra => (false, 1, 80, 80, 52, 89_600, 2_048, true),
    }
}

/// The scale function. Pure and total: identical profiles always produce
/// identical policies.
pub fn policy_for(profile: &ModelProfile) -> RunPolicy {
    let tier = match profile.measured_level {
        Some(level) => tier_for_level(level),
        None => tier_for_params(profile.params_b),
    };
    let (
        uses_planner,
        max_plan_steps,
        max_turns_per_step,
        max_turns,
        max_tools,
        budget_cap,
        max_output_tokens,
        subagents,
    ) = tier_row(tier);
    // 70% of the context window is available as prompt budget (the rest is
    // reserved for generation), capped by the tier's ceiling.
    let prompt_budget_tokens = ((profile.ctx as u64 * 7 / 10) as u32).min(budget_cap);
    RunPolicy {
        tier,
        protocol: Protocol::ConstrainedJson,
        uses_planner,
        max_plan_steps,
        max_turns_per_step,
        max_turns,
        max_tools,
        prompt_budget_tokens,
        max_output_tokens,
        allows_subagents: subagents,
        max_ring: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(params_b: f32, measured_level: Option<u8>) -> ModelProfile {
        ModelProfile {
            params_b,
            quant: "Q4_K_M".to_string(),
            ctx: 4096,
            family: "test".to_string(),
            measured_level,
        }
    }

    #[test]
    fn policy_for_is_deterministic() {
        let p = profile(7.0, Some(3));
        assert_eq!(policy_for(&p), policy_for(&p.clone()));
    }

    #[test]
    fn nano_tier_boundaries() {
        assert_eq!(policy_for(&profile(0.5, None)).tier, Tier::Nano);
        assert_eq!(policy_for(&profile(3.9, None)).tier, Tier::Nano);
        assert_eq!(policy_for(&profile(4.0, None)).tier, Tier::Small);
        assert_eq!(policy_for(&profile(13.1, None)).tier, Tier::Medium);
    }

    #[test]
    fn ring_ceiling_per_tier() {
        assert_eq!(ring_for_tier(Tier::Nano), 0);
        assert_eq!(ring_for_tier(Tier::Small), 1);
        assert_eq!(ring_for_tier(Tier::Medium), 2);
        assert_eq!(ring_for_tier(Tier::Large), 3);
        assert_eq!(ring_for_tier(Tier::Xl), 3);
        assert_eq!(ring_for_tier(Tier::Ultra), 3);
    }

    #[test]
    fn nano_policy_shape() {
        let policy = policy_for(&profile(1.0, None));
        assert_eq!(policy.tier, Tier::Nano);
        assert_eq!(policy.protocol, Protocol::ConstrainedJson);
        assert!(policy.uses_planner);
        assert!(policy.max_tools <= 10);
        assert_eq!(policy.prompt_budget_tokens, 2_800);
        assert!(!policy.allows_subagents);
    }

    #[test]
    fn measured_level_overrides_params() {
        // Downgrade: a 7B that measurably breaks at L2 gets NANO-grade agency.
        let policy = policy_for(&profile(7.0, Some(1)));
        assert_eq!(policy.tier, Tier::Nano);
    }

    #[test]
    fn measured_level_upgrade() {
        // Upgrade: a 1B that measurably completes L4 earns SMALL-grade agency.
        let policy = policy_for(&profile(1.0, Some(4)));
        assert_eq!(policy.tier, Tier::Small);
    }

    #[test]
    fn action_protocol_serde() {
        assert_eq!(
            serde_json::to_string(&ActionProtocol::NativeTools).unwrap(),
            r#""native_tools""#
        );
        assert_eq!(
            serde_json::to_string(&ActionProtocol::ConstrainedJson).unwrap(),
            r#""constrained_json""#
        );
        assert_eq!(
            serde_json::to_string(&ActionProtocol::TextXml).unwrap(),
            r#""text_xml""#
        );
        // New serde spelling round-trips, and the legacy "unified_grammar"
        // alias still deserializes (old traces/bench rows).
        let back: ActionProtocol = serde_json::from_str(r#""constrained_json""#).unwrap();
        assert_eq!(back, ActionProtocol::ConstrainedJson);
        let legacy: ActionProtocol = serde_json::from_str(r#""unified_grammar""#).unwrap();
        assert_eq!(legacy, ActionProtocol::ConstrainedJson);
    }

    #[test]
    fn max_output_tokens_per_tier() {
        assert_eq!(policy_for(&profile(1.0, None)).max_output_tokens, 512);
        assert_eq!(policy_for(&profile(7.0, None)).max_output_tokens, 768);
        assert_eq!(policy_for(&profile(14.0, None)).max_output_tokens, 1_024);
    }

    #[test]
    fn prompt_budget_respects_small_context() {
        // 4096-token context: 70% = 2867, below every tier cap except Nano's.
        let policy = policy_for(&profile(7.0, None));
        assert_eq!(policy.prompt_budget_tokens, 2867);
        // Nano's cap (2800) binds before 70% of context does.
        let nano = policy_for(&profile(1.0, None));
        assert_eq!(nano.prompt_budget_tokens, 2800);
    }
}

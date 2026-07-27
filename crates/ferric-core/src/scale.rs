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

/// Characters of a single tool output the model sees; the trace always gets
/// the full output (ADR-002). Seeded from Animus `max_tool_output_chars`.
///
/// It lives here, in the crate everything depends on, because three separate
/// consumers need it and none of them may depend on each other: the registry
/// that applies it (`ferric-tools`), the event that records it
/// (`ferric-trace`), and the projector that reapplies it when rebuilding a
/// context window from a trace (`ferric-loop`). ADR-093.
pub const DEFAULT_TRUNCATION_LIMIT: usize = 4_000;

/// Serde default for the cap recorded in a trace. A `policy_selected` line
/// written before ADR-093 has no such key, and the runs that wrote those
/// lines used exactly this value — so defaulting here reproduces them rather
/// than guessing at them.
pub fn default_truncation_limit() -> usize {
    DEFAULT_TRUNCATION_LIMIT
}

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
    Plan,
}

/// The string that identifies a protocol in `model_profiles.json`.
///
/// Profiles are keyed on `(model, protocol)` where protocol is a free-form
/// `String`, and until sprint 98 four call sites — one reader in `ferric query`
/// and three writers in bench/toolbench — each independently wrote
/// `format!("{protocol:?}")`. They agreed only because they all happened to
/// reach for `Debug`.
///
/// That is a bad thing to leave implicit, because the failure is silent:
/// renaming a variant, or switching one site to `Display` or serde, makes
/// `read_profile` miss, and a miss is deliberately **a safe no-op** (ADR-029) —
/// so the model runs at its params-derived tier instead of its measured one and
/// nothing reports a problem. `tier_table_snapshot`-style breakage would be
/// obvious; this would not.
///
/// One definition, and a test pinning the exact strings, so a rename breaks
/// loudly instead of degrading quietly.
pub fn protocol_key(protocol: ActionProtocol) -> String {
    match protocol {
        ActionProtocol::NativeTools => "NativeTools",
        ActionProtocol::ConstrainedJson => "ConstrainedJson",
        ActionProtocol::TextXml => "TextXml",
        ActionProtocol::Plan => "Plan",
    }
    .to_string()
}

/// Everything the agent loop needs to know about how to run a given model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunPolicy {
    pub tier: Tier,
    /// Why this run is at this `tier` (ADR-098). Part of the decision, not
    /// metadata about it: an earned tier and an asked-for tier produce
    /// identical budgets, so without this the record cannot tell them apart.
    #[serde(default)]
    pub tier_source: TierSource,
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
    #[serde(default = "default_compact_trigger_fraction")]
    pub compact_trigger_fraction: f32,
    #[serde(default = "default_compact_keep_last_turns")]
    pub compact_keep_last_turns: u8,
}

fn default_compact_trigger_fraction() -> f32 {
    0.85
}
fn default_compact_keep_last_turns() -> u8 {
    2
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
/// max_turns, max_tools, prompt_budget_cap, max_output_tokens, allows_subagents,
/// compact_trigger_fraction, compact_keep_last_turns).
#[allow(clippy::type_complexity)]
fn tier_row(tier: Tier) -> (bool, u8, u8, u8, u8, u32, u32, bool, f32, u8) {
    match tier {
        Tier::Nano => (true, 3, 5, 15, 10, 2_800, 512, false, 0.75, 1),
        Tier::Small => (true, 5, 4, 20, 14, 5_600, 768, false, 0.80, 2),
        Tier::Medium => (false, 1, 25, 25, 20, 11_200, 1_024, false, 0.85, 2),
        Tier::Large => (false, 1, 40, 40, 28, 22_400, 1_536, true, 0.85, 3),
        Tier::Xl => (false, 1, 60, 60, 36, 44_800, 2_048, true, 0.85, 3),
        Tier::Ultra => (false, 1, 80, 80, 52, 89_600, 2_048, true, 0.90, 4),
    }
}

/// Why a run ended up at the tier it did (ADR-098).
///
/// The tier decides turn/tool budgets, prompt and output ceilings, planner use,
/// subagents, and the tool-ring ceiling — so "which tier" is the single most
/// consequential fact about a run. Recording only the *answer* made two very
/// different situations identical in the record: a tier a model **earned** on
/// the benchmark ladder, and a tier an operator simply asked for.
/// Fieldless on purpose: it round-trips losslessly through the one-word label
/// the trace stores, and the measured *level* itself already lives in the
/// profile store. What the record needs is the distinction, not the number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TierSource {
    /// From a persisted `measured_level` — the model demonstrated it (ADR-029).
    Measured,
    /// Derived from the declared parameter count: the prior, not a measurement.
    #[default]
    Params,
    /// Asked for outright by the operator (`--tier` / config `tier`).
    Override,
}

impl TierSource {
    /// Stable label for traces and diagnostics.
    pub fn label(&self) -> &'static str {
        match self {
            TierSource::Measured => "measured",
            TierSource::Params => "params",
            TierSource::Override => "override",
        }
    }

    /// Inverse of [`label`](Self::label). An unrecognized label reads back as
    /// `Params` — the conservative answer, and what a pre-ADR-098 trace meant.
    pub fn from_label(s: &str) -> Self {
        match s {
            "measured" => TierSource::Measured,
            "override" => TierSource::Override,
            _ => TierSource::Params,
        }
    }
}

/// Resolve the tier and *why*. Precedence: an explicit operator override wins,
/// then a measured level, then the parameter-count prior.
///
/// The override sits on top rather than replacing the ladder because
/// `params_b` is a **fact about the model**, not a dial. Before ADR-098 the
/// only manual route to a tier was to misstate that fact — claiming 30B to
/// reach `Large` — which corrupted an input the trace and the profile store
/// both rely on, and left "measured Large" and "claimed 30B" indistinguishable
/// afterwards. An override is an honest, separately recorded operator decision;
/// ADR-006 rules out runtime *heuristics*, and a config-supplied choice is not
/// one.
pub fn tier_decision(profile: &ModelProfile, override_tier: Option<Tier>) -> (Tier, TierSource) {
    match (override_tier, profile.measured_level) {
        (Some(t), _) => (t, TierSource::Override),
        (None, Some(level)) => (tier_for_level(level), TierSource::Measured),
        (None, None) => (tier_for_params(profile.params_b), TierSource::Params),
    }
}

/// The scale function. Pure and total: identical profiles always produce
/// identical policies.
pub fn policy_for(profile: &ModelProfile) -> RunPolicy {
    policy_for_with_override(profile, None)
}

/// `policy_for` with an explicit operator tier override (ADR-098).
pub fn policy_for_with_override(profile: &ModelProfile, override_tier: Option<Tier>) -> RunPolicy {
    let (tier, tier_source) = tier_decision(profile, override_tier);
    let (
        uses_planner,
        max_plan_steps,
        max_turns_per_step,
        max_turns,
        max_tools,
        budget_cap,
        max_output_tokens,
        subagents,
        compact_trigger_fraction,
        compact_keep_last_turns,
    ) = tier_row(tier);
    // 70% of the context window is available as prompt budget (the rest is
    // reserved for generation), capped by the tier's ceiling.
    let prompt_budget_tokens = ((profile.ctx as u64 * 7 / 10) as u32).min(budget_cap);
    RunPolicy {
        tier,
        tier_source,
        uses_planner,
        max_plan_steps,
        max_turns_per_step,
        max_turns,
        max_tools,
        prompt_budget_tokens,
        max_output_tokens,
        allows_subagents: subagents,
        max_ring: None,
        compact_trigger_fraction,
        compact_keep_last_turns,
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

    // --- ADR-098: an operator tier, and saying so ---

    /// The complaint this answers: before `--tier`, the only manual route to a
    /// tier was to misstate `params_b`. That is a *fact* about the model, so
    /// the dishonest route corrupted an input the profile store and trace both
    /// rely on — and afterwards nothing could tell an earned tier from a
    /// claimed one.
    #[test]
    fn an_override_beats_both_measurement_and_size() {
        // A 7B measured at level 6 would otherwise be Large.
        let measured = profile(7.0, Some(6));
        assert_eq!(policy_for(&measured).tier, Tier::Large);

        // Held DOWN — the override works in both directions, which the
        // params route never could without claiming the model was tiny.
        let held = policy_for_with_override(&measured, Some(Tier::Nano));
        assert_eq!(held.tier, Tier::Nano);
        assert_eq!(held.tier_source, TierSource::Override);

        // Lifted UP, with no lie about size.
        let small = profile(7.0, None);
        assert_eq!(policy_for(&small).tier, Tier::Small);
        let lifted = policy_for_with_override(&small, Some(Tier::Large));
        assert_eq!(lifted.tier, Tier::Large);
        assert_eq!(lifted.tier_source, TierSource::Override);
    }

    /// The source has to distinguish the three routes, or the override just
    /// recreates the ambiguity it was added to remove.
    #[test]
    fn tier_source_records_which_route_was_taken() {
        assert_eq!(
            tier_decision(&profile(7.0, Some(6)), None).1,
            TierSource::Measured
        );
        assert_eq!(
            tier_decision(&profile(7.0, None), None).1,
            TierSource::Params
        );
        assert_eq!(
            tier_decision(&profile(7.0, Some(6)), Some(Tier::Large)).1,
            TierSource::Override
        );
    }

    /// Same tier, different provenance: the budgets are identical, so the
    /// label is the ONLY thing separating them. This is the whole argument for
    /// recording it.
    #[test]
    fn an_earned_large_and_an_asked_for_large_differ_only_in_the_label() {
        let earned = policy_for(&profile(7.0, Some(6)));
        let asked = policy_for_with_override(&profile(7.0, None), Some(Tier::Large));

        assert_eq!(earned.tier, asked.tier);
        assert_eq!(earned.max_turns, asked.max_turns);
        assert_eq!(earned.max_tools, asked.max_tools);
        assert_eq!(earned.max_output_tokens, asked.max_output_tokens);
        assert_ne!(
            earned.tier_source, asked.tier_source,
            "identical budgets — provenance is the only distinguishing field"
        );
    }

    #[test]
    fn tier_source_labels_round_trip() {
        for s in [
            TierSource::Measured,
            TierSource::Params,
            TierSource::Override,
        ] {
            assert_eq!(TierSource::from_label(s.label()), s);
        }
        // An unknown label reads back conservatively, which is also what a
        // pre-ADR-098 trace means.
        assert_eq!(TierSource::from_label("who knows"), TierSource::Params);
    }

    /// `policy_for` must stay exactly what it was for every existing caller.
    #[test]
    fn policy_for_is_unchanged_without_an_override() {
        assert_eq!(
            policy_for(&profile(7.0, Some(6))),
            policy_for_with_override(&profile(7.0, Some(6)), None)
        );
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

    /// Pins the on-disk profile keys. These strings are a **persistence
    /// contract**: `benchmarks/model_profiles.json` already contains
    /// `"protocol": "ConstrainedJson"`, and `ferric query` looks a record up by
    /// this exact value. Changing one silently orphans every stored profile —
    /// `read_profile` misses, the miss is a documented safe no-op (ADR-029), and
    /// the model quietly drops to its params-derived tier.
    ///
    /// So this test is not restating the implementation. It is the thing that
    /// makes a variant rename a visible failure instead of a silent capability
    /// regression.
    #[test]
    fn protocol_keys_match_what_is_already_on_disk() {
        assert_eq!(
            protocol_key(ActionProtocol::ConstrainedJson),
            "ConstrainedJson"
        );
        assert_eq!(protocol_key(ActionProtocol::NativeTools), "NativeTools");
        assert_eq!(protocol_key(ActionProtocol::TextXml), "TextXml");
        assert_eq!(protocol_key(ActionProtocol::Plan), "Plan");
    }

    /// The keys were `format!("{:?}")` at every call site before sprint 98.
    /// Existing profiles were written that way, so the shared function must keep
    /// producing exactly that — otherwise unifying the sites would itself have
    /// been the orphaning change it was meant to prevent.
    #[test]
    fn protocol_key_still_agrees_with_the_debug_format_it_replaced() {
        for p in [
            ActionProtocol::ConstrainedJson,
            ActionProtocol::NativeTools,
            ActionProtocol::TextXml,
            ActionProtocol::Plan,
        ] {
            assert_eq!(protocol_key(p), format!("{p:?}"), "key drifted for {p:?}");
        }
    }
}

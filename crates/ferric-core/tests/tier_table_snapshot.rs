//! Snapshot of the scale function over the real local GGUF fleet.
//!
//! Any tier-table change shows up here as a visible, reviewed diff.

use ferric_core::{ModelProfile, Protocol, Tier, policy_for};

struct Expected {
    family: &'static str,
    params_b: f32,
    ctx: u32,
    tier: Tier,
    uses_planner: bool,
    max_plan_steps: u8,
    max_turns_per_step: u8,
    max_turns: u8,
    max_tools: u8,
    prompt_budget_tokens: u32,
}

fn fleet_profile(family: &str, params_b: f32, ctx: u32) -> ModelProfile {
    ModelProfile {
        params_b,
        quant: "Q4_K_M".to_string(),
        ctx,
        family: family.to_string(),
        measured_level: None,
    }
}

#[rustfmt::skip]
fn fleet_expectations() -> Vec<Expected> {
    let row = |family, params_b, ctx, tier, uses_planner, max_plan_steps,
               max_turns_per_step, max_turns, max_tools, prompt_budget_tokens| Expected {
        family, params_b, ctx, tier, uses_planner, max_plan_steps,
        max_turns_per_step, max_turns, max_tools, prompt_budget_tokens,
    };
    vec![
        row("llama-3.2",     1.0, 4096, Tier::Nano,   true,  3, 5,  15, 6,  2_800),
        row("qwen2.5-coder", 7.0, 4096, Tier::Small,  true,  5, 4,  20, 10, 2_867),
        row("command-r7b",   7.0, 4096, Tier::Small,  true,  5, 4,  20, 10, 2_867),
        row("qwen3-vl",      8.0, 4096, Tier::Small,  true,  5, 4,  20, 10, 2_867),
        row("qwen2.5-coder", 14.0, 4096, Tier::Medium, false, 1, 25, 25, 16, 2_867),
    ]
}

#[test]
fn tier_table_snapshot() {
    for e in fleet_expectations() {
        let policy = policy_for(&fleet_profile(e.family, e.params_b, e.ctx));
        let label = format!("{} {}B", e.family, e.params_b);
        assert_eq!(policy.tier, e.tier, "{label} tier");
        assert_eq!(
            policy.protocol,
            Protocol::ConstrainedJson,
            "{label} protocol"
        );
        assert_eq!(policy.uses_planner, e.uses_planner, "{label} planner");
        assert_eq!(
            policy.max_plan_steps, e.max_plan_steps,
            "{label} plan steps"
        );
        assert_eq!(
            policy.max_turns_per_step, e.max_turns_per_step,
            "{label} turns/step"
        );
        assert_eq!(policy.max_turns, e.max_turns, "{label} max turns");
        assert_eq!(policy.max_tools, e.max_tools, "{label} tool cap");
        assert_eq!(
            policy.prompt_budget_tokens, e.prompt_budget_tokens,
            "{label} prompt budget"
        );
        assert!(
            !policy.allows_subagents,
            "{label} subagents stay off <= Medium"
        );
    }
}

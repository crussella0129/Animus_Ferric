use serde::Serialize;

use ferric_guard::{PermissionLevel, Workspace};

/// Declarative description of a tool: what the model sees (name, description,
/// schema) plus what the harness enforces (permission level, ring).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub permission: PermissionLevel,
    /// Tool-vocabulary ring (0 = the always-on navigate/mutate core). The loop
    /// includes rings `0..=ring_for_tier(tier)` and the registry trims from the
    /// outer ring first, so the active rings = the model's grammar.
    pub ring: u8,
}

/// Execution context handed to tools. Tools must resolve every path they
/// touch through `workspace` (defense in depth — the registry has already
/// checked declared targets, but tools own their own discipline too).
pub struct ToolCtx<'a> {
    pub workspace: &'a Workspace,
}

/// A tool implementation.
///
/// `run` is synchronous in s0 (three file tools need no concurrency). The
/// registry's execute chokepoint is the ONLY call site, so converting this to
/// async for s1's exec/bash tools (timeout, cancellation) is a one-file
/// change — do not pre-pay that complexity here.
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;

    /// The workspace-relative or absolute paths this call will touch, parsed
    /// from `args`. The registry boundary-resolves and permission-checks every
    /// one BEFORE `run` is invoked. Default: the `"path"` string field, if any.
    fn target_paths(&self, args: &serde_json::Value) -> Vec<String> {
        args.get("path")
            .and_then(|v| v.as_str())
            .map(|s| vec![s.to_string()])
            .unwrap_or_default()
    }

    /// Execute. `Ok` is the full (untruncated) success output; `Err` is the
    /// full error text. Truncation for the model happens in the registry.
    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String>;
}

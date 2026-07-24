use std::collections::BTreeMap;
use std::time::Instant;

use ferric_core::{RunPolicy, ring_for_tier};
use ferric_guard::{Decision, Workspace, check_with_ignore};
use tracing::{debug, warn};

use crate::spec::{Tool, ToolCtx, ToolSpec};

/// Characters of tool output the model sees; the trace always gets the full
/// output (ADR-002). Seeded from Animus `max_tool_output_chars`.
pub const DEFAULT_TRUNCATION_LIMIT: usize = 4_000;

/// A tool's output, split at the chokepoint: `full` goes to the trace,
/// `for_model` (truncated) goes back into the prompt.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutput {
    pub full: String,
    pub for_model: String,
    pub is_error: bool,
}

/// One guard decision made at the chokepoint, kept so the loop can trace
/// exactly what was checked and why. `rule`/`matched` are set on denials.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckRecord {
    pub path: std::path::PathBuf,
    /// "allow" | "deny"
    pub decision: String,
    pub rule: Option<String>,
    pub matched: Option<String>,
}

impl CheckRecord {
    fn allow(path: std::path::PathBuf) -> Self {
        Self {
            path,
            decision: "allow".to_string(),
            rule: None,
            matched: None,
        }
    }

    fn deny(path: std::path::PathBuf, rule: &str, matched: impl Into<String>) -> Self {
        Self {
            path,
            decision: "deny".to_string(),
            rule: Some(rule.to_string()),
            matched: Some(matched.into()),
        }
    }
}

/// The outcome of `Registry::execute`. A `Denied` outcome means the tool
/// handler never ran. Both terminal variants carry the guard's per-target
/// `CheckRecord`s for tracing.
#[derive(Debug, Clone, PartialEq)]
pub enum ExecuteOutcome {
    Completed {
        output: ToolOutput,
        duration_ms: u64,
        checks: Vec<CheckRecord>,
    },
    Denied {
        reason: String,
        checks: Vec<CheckRecord>,
    },
    UnknownTool {
        name: String,
    },
}

/// Tool registry. `execute` is the single chokepoint every tool call flows
/// through: boundary resolution + permission check (before the handler),
/// timing, and the full-vs-truncated output split (after it).
pub struct Registry {
    // BTreeMap keeps enumeration deterministically sorted (ADR-008).
    tools: BTreeMap<String, Box<dyn Tool>>,
    truncation_limit: usize,
}

impl Registry {
    pub fn new() -> Self {
        Self::with_truncation_limit(DEFAULT_TRUNCATION_LIMIT)
    }

    pub fn with_truncation_limit(truncation_limit: usize) -> Self {
        Self {
            tools: BTreeMap::new(),
            truncation_limit,
        }
    }

    /// The configured model-facing output cap. The loop reads this to keep its
    /// projector's truncation in step with the registry's.
    pub fn truncation_limit(&self) -> usize {
        self.truncation_limit
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.spec().name.clone(), tool);
    }

    /// The permission level a registered tool declares, or `None` if unknown.
    /// Used by the loop's accept-edits gate (ADR-070) to decide which calls to
    /// preview to the human (only `Write`/`Execute` — mutating ones).
    pub fn permission_of(&self, name: &str) -> Option<ferric_guard::PermissionLevel> {
        self.tools.get(name).map(|t| t.spec().permission)
    }

    /// The specs a given run policy may use (the rings model): keep tools whose
    /// `ring <= ring_for_tier(policy.tier)`, and when over `policy.max_tools`
    /// **trim from the outer ring first** (priority by `(ring asc, name)`) so the
    /// core is never dropped — then return the kept set name-sorted (ADR-008).
    /// The loop builds the action grammar from exactly this set, so the active
    /// rings ARE the model's grammar. (Replaces the old alphabetical `.take` cap,
    /// which could silently drop an essential core tool — e.g. `write_file`.)
    pub fn tools_for_policy(&self, policy: &RunPolicy) -> Vec<ToolSpec> {
        // The tier sets the ceiling; an explicit `--max-ring` (policy.max_ring)
        // can only lower it further (restrict-only — expansion is earned via
        // measured_level, ADR-028/019).
        let ceiling = ring_for_tier(policy.tier).min(policy.max_ring.unwrap_or(u8::MAX));
        let mut specs: Vec<ToolSpec> = self
            .tools
            .values()
            .map(|t| t.spec())
            .filter(|spec| spec.ring <= ceiling)
            .collect();
        // Priority by (ring asc, name): a cap sheds the highest rings first.
        specs.sort_by(|a, b| a.ring.cmp(&b.ring).then_with(|| a.name.cmp(&b.name)));
        specs.truncate(policy.max_tools as usize);
        // Deterministic presentation by name (ADR-008).
        specs.sort_by(|a, b| a.name.cmp(&b.name));
        specs
    }

    /// Execute `name` with `args` inside `workspace`. The guard check runs
    /// against every declared target path BEFORE the handler; a denial means
    /// the handler is never invoked.
    pub fn execute(
        &self,
        workspace: &Workspace,
        name: &str,
        args: &serde_json::Value,
        taint_set: &ferric_guard::TaintSet,
        sink_policy: &ferric_guard::SinkPolicy,
    ) -> ExecuteOutcome {
        let Some(tool) = self.tools.get(name) else {
            return ExecuteOutcome::UnknownTool {
                name: name.to_string(),
            };
        };
        let spec = tool.spec();

        let mut checks = Vec::new();
        for target in tool.target_paths(args) {
            let resolved = match workspace.resolve(&target) {
                Ok(path) => path,
                Err(e) => {
                    warn!(tool = name, target = %target, error = %e, "guard denied: outside workspace boundary");
                    checks.push(CheckRecord::deny(target.into(), "boundary", e.to_string()));
                    return ExecuteOutcome::Denied {
                        reason: format!("boundary: {e}"),
                        checks,
                    };
                }
            };
            if let Decision::Deny(reason) = check_with_ignore(
                spec.permission,
                &resolved,
                workspace.root(),
                workspace.ignore(),
            ) {
                warn!(tool = name, path = %resolved.display(), rule = %reason.rule, matched = %reason.matched, "guard denied: permission check");
                let detail = format!("permission: {} matched {}", reason.rule, reason.matched);
                checks.push(CheckRecord::deny(resolved, reason.rule, &reason.matched));
                return ExecuteOutcome::Denied {
                    reason: detail,
                    checks,
                };
            }
            checks.push(CheckRecord::allow(resolved));
        }

        for cmd in tool.target_commands(args) {
            if let Decision::Deny(reason) = ferric_guard::check_command(&cmd) {
                warn!(tool = name, command = %cmd, rule = %reason.rule, matched = %reason.matched, "guard denied: command denylist");
                let detail = format!("permission: {} matched {}", reason.rule, reason.matched);
                checks.push(CheckRecord::deny(
                    std::path::PathBuf::from(&cmd),
                    reason.rule,
                    &reason.matched,
                ));
                return ExecuteOutcome::Denied {
                    reason: detail,
                    checks,
                };
            }
            checks.push(CheckRecord::allow(std::path::PathBuf::from(&cmd)));
        }

        let is_tainted = taint_set.args_tainted(args);
        match sink_policy.decide(spec.permission, is_tainted) {
            ferric_guard::SinkDecision::Allow => {}
            ferric_guard::SinkDecision::Warn => {
                warn!(
                    tool = name,
                    permission = ?spec.permission,
                    "sink policy: tainted data reaching a {:?} sink (warn mode; proceeding)",
                    spec.permission
                );
            }
            ferric_guard::SinkDecision::RequireApproval => {
                // Fall back to deny since human approval is not wired
                warn!(tool = name, permission = ?spec.permission, "sink policy: tainted data at sink; require-approval not wired, denying");
                return ExecuteOutcome::Denied {
                    reason: "sink policy: require approval not implemented".to_string(),
                    checks,
                };
            }
            ferric_guard::SinkDecision::Deny => {
                warn!(tool = name, permission = ?spec.permission, "sink policy: tainted data denied at sink");
                return ExecuteOutcome::Denied {
                    reason: "sink policy: tainted data denied at sink".to_string(),
                    checks,
                };
            }
        }

        let ctx = ToolCtx { workspace };
        let started = Instant::now();
        let result = tool.run(&ctx, args);
        let duration_ms = started.elapsed().as_millis() as u64;

        let (full, is_error) = match result {
            Ok(output) => (output, false),
            Err(error) => (error, true),
        };
        debug!(tool = name, is_error, duration_ms, "tool handler returned");
        let for_model = truncate_chars(&full, self.truncation_limit);
        ExecuteOutcome::Completed {
            output: ToolOutput {
                full,
                for_model,
                is_error,
            },
            duration_ms,
            checks,
        }
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

/// The model-facing view of a tool result: at most `limit` chars, with an
/// explicit marker so the model knows it is looking at a prefix.
///
/// Public because the loop's `TraceProjector` — not the registry — is what
/// actually assembles the context window (sprint 44). The projector rebuilds
/// messages from trace events, and the trace deliberately stores the *full*
/// output, so the projector has to apply this itself. Both callers must use the
/// same function or the two views drift.
pub fn truncate_for_model(text: &str, limit: usize) -> String {
    truncate_chars(text, limit)
}

fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let kept: String = text.chars().take(limit).collect();
    format!("{kept}\n[... output truncated for model; full output in trace]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    use ferric_core::{ModelProfile, policy_for};
    use ferric_guard::PermissionLevel;
    use serde_json::json;

    struct DummyTool {
        name: String,
        permission: PermissionLevel,
        output_len: usize,
        ran: std::sync::Arc<AtomicBool>,
        ring: u8,
    }

    impl Tool for DummyTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: self.name.clone(),
                description: "dummy".to_string(),
                input_schema: json!({"type": "object"}),
                permission: self.permission,
                ring: self.ring,
            }
        }

        fn run(&self, _ctx: &ToolCtx<'_>, _args: &serde_json::Value) -> Result<String, String> {
            self.ran.store(true, Ordering::SeqCst);
            Ok("y".repeat(self.output_len))
        }
    }

    fn dummy(
        name: &str,
        permission: PermissionLevel,
        output_len: usize,
    ) -> (DummyTool, std::sync::Arc<AtomicBool>) {
        let ran = std::sync::Arc::new(AtomicBool::new(false));
        (
            DummyTool {
                name: name.to_string(),
                permission,
                output_len,
                ran: ran.clone(),
                ring: 0,
            },
            ran,
        )
    }

    fn dummy_ring(name: &str, ring: u8) -> DummyTool {
        DummyTool {
            name: name.to_string(),
            permission: PermissionLevel::Read,
            output_len: 1,
            ran: std::sync::Arc::new(AtomicBool::new(false)),
            ring,
        }
    }

    fn temp_workspace() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path()).unwrap();
        (dir, ws)
    }

    #[test]
    fn execute_blocks_on_deny() {
        let (_dir, ws) = temp_workspace();
        let (tool, ran) = dummy("writer", PermissionLevel::Write, 4);
        let mut registry = Registry::new();
        registry.register(Box::new(tool));

        let outcome = registry.execute(
            &ws,
            "writer",
            &json!({"path": ".git/config"}),
            &ferric_guard::TaintSet::new(),
            &ferric_guard::SinkPolicy::deny(),
        );
        assert!(
            matches!(outcome, ExecuteOutcome::Denied { ref reason, .. } if reason.contains(".git")),
            "expected Denied, got {outcome:?}"
        );
        assert!(!ran.load(Ordering::SeqCst), "handler must not run on deny");
    }

    #[test]
    fn check_records_on_allow() {
        let (_dir, ws) = temp_workspace();
        let (tool, _) = dummy("writer", PermissionLevel::Write, 4);
        let mut registry = Registry::new();
        registry.register(Box::new(tool));

        match registry.execute(
            &ws,
            "writer",
            &json!({"path": "notes.md"}),
            &ferric_guard::TaintSet::new(),
            &ferric_guard::SinkPolicy::deny(),
        ) {
            ExecuteOutcome::Completed { checks, .. } => {
                assert_eq!(checks.len(), 1);
                assert_eq!(checks[0].decision, "allow");
                assert!(checks[0].rule.is_none());
                assert!(checks[0].path.ends_with("notes.md"));
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[test]
    fn check_records_on_deny() {
        let (_dir, ws) = temp_workspace();
        let (tool, _) = dummy("writer", PermissionLevel::Write, 4);
        let mut registry = Registry::new();
        registry.register(Box::new(tool));

        match registry.execute(
            &ws,
            "writer",
            &json!({"path": ".git/config"}),
            &ferric_guard::TaintSet::new(),
            &ferric_guard::SinkPolicy::deny(),
        ) {
            ExecuteOutcome::Denied { checks, .. } => {
                assert_eq!(checks.len(), 1);
                assert_eq!(checks[0].decision, "deny");
                assert_eq!(checks[0].rule.as_deref(), Some("denied_write_segment"));
                assert_eq!(checks[0].matched.as_deref(), Some(".git"));
            }
            other => panic!("expected Denied, got {other:?}"),
        }
    }

    #[test]
    fn ferric_dir_write_denied() {
        let (_dir, ws) = temp_workspace();
        let (tool, ran) = dummy("writer", PermissionLevel::Write, 4);
        let mut registry = Registry::new();
        registry.register(Box::new(tool));

        let outcome = registry.execute(
            &ws,
            "writer",
            &json!({"path": ".ferric/trace/x.jsonl"}),
            &ferric_guard::TaintSet::new(),
            &ferric_guard::SinkPolicy::deny(),
        );
        match outcome {
            ExecuteOutcome::Denied { checks, .. } => {
                assert_eq!(checks[0].rule.as_deref(), Some("denied_write_segment"));
                assert_eq!(checks[0].matched.as_deref(), Some(".ferric"));
            }
            other => panic!("expected Denied, got {other:?}"),
        }
        assert!(!ran.load(Ordering::SeqCst));
    }

    #[test]
    fn output_truncation_preserves_full() {
        let (_dir, ws) = temp_workspace();
        let (tool, _) = dummy("bigout", PermissionLevel::Read, 1_000_000);
        let mut registry = Registry::new();
        registry.register(Box::new(tool));

        match registry.execute(
            &ws,
            "bigout",
            &json!({}),
            &ferric_guard::TaintSet::new(),
            &ferric_guard::SinkPolicy::deny(),
        ) {
            ExecuteOutcome::Completed { output, .. } => {
                assert_eq!(output.full.len(), 1_000_000);
                assert!(output.for_model.chars().count() <= DEFAULT_TRUNCATION_LIMIT + 100);
                assert!(output.for_model.contains("truncated"));
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[test]
    fn tools_for_policy_sorted_and_capped() {
        let mut registry = Registry::new();
        // Register 10 tools in shuffled order; NANO max_tools is 6.
        for name in [
            "zeta", "echo", "mike", "alpha", "kilo", "bravo", "xray", "golf", "tango", "delta",
        ] {
            let (tool, _) = dummy(name, PermissionLevel::Read, 1);
            registry.register(Box::new(tool));
        }
        let nano = policy_for(&ModelProfile {
            params_b: 1.0,
            quant: "Q4_K_M".to_string(),
            ctx: 4096,
            family: "test".to_string(),
            measured_level: None,
        });
        let specs = registry.tools_for_policy(&nano);
        assert_eq!(specs.len(), nano.max_tools as usize);
        let names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "tool list must be alphabetically sorted");
        // Two calls, identical ordering.
        let again: Vec<String> = registry
            .tools_for_policy(&nano)
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, again.iter().map(String::as_str).collect::<Vec<_>>());
    }

    #[test]
    fn tools_for_policy_trims_outer_ring_first() {
        let mut registry = Registry::new();
        let core: Vec<String> = (0..10).map(|i| format!("core_{i}")).collect();
        for n in &core {
            registry.register(Box::new(dummy_ring(n, 0)));
        }
        for i in 0..10 {
            registry.register(Box::new(dummy_ring(&format!("outer_{i}"), 1)));
        }
        // Small: ring ceiling 1 (admits both rings), max_tools 14 < 20 → must trim.
        let small = policy_for(&ModelProfile {
            params_b: 8.0,
            quant: "Q4_K_M".to_string(),
            ctx: 4096,
            family: "t".to_string(),
            measured_level: None,
        });
        let names: Vec<String> = registry
            .tools_for_policy(&small)
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names.len(), small.max_tools as usize, "capped to max_tools");
        // Every Ring-0 (core) tool survives the cap; only Ring-1 tools are shed.
        for c in &core {
            assert!(
                names.contains(c),
                "core tool {c} must survive the cap: {names:?}"
            );
        }
        // Deterministic, name-sorted (ADR-008).
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "result must be name-sorted");

        // A Nano model (ring ceiling 0) sees ONLY the core ring, never the outer.
        let nano = policy_for(&ModelProfile {
            params_b: 1.0,
            quant: "Q4_K_M".to_string(),
            ctx: 4096,
            family: "t".to_string(),
            measured_level: None,
        });
        let nano_names: Vec<String> = registry
            .tools_for_policy(&nano)
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert!(
            nano_names.iter().all(|n| n.starts_with("core_")),
            "Nano sees only Ring 0: {nano_names:?}"
        );
    }

    #[test]
    fn tools_for_policy_max_ring_override_caps() {
        let mut registry = Registry::new();
        for n in ["a_core", "b_core", "c_core"] {
            registry.register(Box::new(dummy_ring(n, 0)));
        }
        for n in ["x_outer", "y_outer"] {
            registry.register(Box::new(dummy_ring(n, 1)));
        }
        // Small → ring ceiling 1 → both rings admitted (5 tools).
        let base = policy_for(&ModelProfile {
            params_b: 8.0,
            quant: "Q4_K_M".to_string(),
            ctx: 4096,
            family: "t".to_string(),
            measured_level: None,
        });
        assert_eq!(base.max_ring, None, "policy_for leaves the override unset");
        assert_eq!(
            registry.tools_for_policy(&base).len(),
            5,
            "None = tier default"
        );

        // --max-ring 0 → only the core ring, even though the tier allows ring 1.
        let mut capped = base.clone();
        capped.max_ring = Some(0);
        let names: Vec<String> = registry
            .tools_for_policy(&capped)
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert!(
            names.iter().all(|n| n.ends_with("_core")),
            "max_ring 0 restricts to the core: {names:?}"
        );
        assert_eq!(names.len(), 3);

        // --max-ring above the tier ceiling is a no-op (capped by the tier).
        let mut high = base.clone();
        high.max_ring = Some(5);
        assert_eq!(
            registry.tools_for_policy(&high).len(),
            5,
            "override only lowers"
        );
    }

    #[test]
    fn unknown_tool_outcome() {
        let (_dir, ws) = temp_workspace();
        let registry = Registry::new();
        let outcome = registry.execute(
            &ws,
            "nope",
            &json!({}),
            &ferric_guard::TaintSet::new(),
            &ferric_guard::SinkPolicy::deny(),
        );
        assert!(matches!(outcome, ExecuteOutcome::UnknownTool { ref name } if name == "nope"));
    }
}

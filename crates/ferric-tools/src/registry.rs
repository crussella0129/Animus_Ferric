use std::collections::BTreeMap;
use std::time::Instant;

use ferric_core::RunPolicy;
use ferric_guard::{Decision, Workspace, check};

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

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.spec().name.clone(), tool);
    }

    /// The specs a given run policy may use: filtered by minimum tier, sorted
    /// alphabetically (BTreeMap order), capped at `policy.max_tools`.
    pub fn tools_for_policy(&self, policy: &RunPolicy) -> Vec<ToolSpec> {
        self.tools
            .values()
            .map(|t| t.spec())
            .filter(|spec| spec.min_tier <= policy.tier)
            .take(policy.max_tools as usize)
            .collect()
    }

    /// Execute `name` with `args` inside `workspace`. The guard check runs
    /// against every declared target path BEFORE the handler; a denial means
    /// the handler is never invoked.
    pub fn execute(
        &self,
        workspace: &Workspace,
        name: &str,
        args: &serde_json::Value,
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
                    checks.push(CheckRecord::deny(target.into(), "boundary", e.to_string()));
                    return ExecuteOutcome::Denied {
                        reason: format!("boundary: {e}"),
                        checks,
                    };
                }
            };
            if let Decision::Deny(reason) = check(spec.permission, &resolved) {
                let detail = format!("permission: {} matched {}", reason.rule, reason.matched);
                checks.push(CheckRecord::deny(resolved, reason.rule, reason.matched));
                return ExecuteOutcome::Denied {
                    reason: detail,
                    checks,
                };
            }
            checks.push(CheckRecord::allow(resolved));
        }

        let ctx = ToolCtx { workspace };
        let started = Instant::now();
        let result = tool.run(&ctx, args);
        let duration_ms = started.elapsed().as_millis() as u64;

        let (full, is_error) = match result {
            Ok(output) => (output, false),
            Err(error) => (error, true),
        };
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
    }

    impl Tool for DummyTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: self.name.clone(),
                description: "dummy".to_string(),
                input_schema: json!({"type": "object"}),
                permission: self.permission,
                min_tier: ferric_core::Tier::Nano,
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
            },
            ran,
        )
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

        let outcome = registry.execute(&ws, "writer", &json!({"path": ".git/config"}));
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

        match registry.execute(&ws, "writer", &json!({"path": "notes.md"})) {
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

        match registry.execute(&ws, "writer", &json!({"path": ".git/config"})) {
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

        let outcome = registry.execute(&ws, "writer", &json!({"path": ".ferric/trace/x.jsonl"}));
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

        match registry.execute(&ws, "bigout", &json!({})) {
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
    fn unknown_tool_outcome() {
        let (_dir, ws) = temp_workspace();
        let registry = Registry::new();
        let outcome = registry.execute(&ws, "nope", &json!({}));
        assert!(matches!(outcome, ExecuteOutcome::UnknownTool { ref name } if name == "nope"));
    }
}

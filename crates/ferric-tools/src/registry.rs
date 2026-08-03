use std::collections::BTreeMap;
use std::fmt;
use std::time::Instant;

use ferric_core::{RunPolicy, ring_for_tier};
use ferric_guard::{Decision, Workspace, check_with_ignore};
use tracing::{debug, warn};

use crate::control::{
    ControlCapability, ControlFailure, ControlFailureKind, ControlMetadata, PrepareCtx,
    PrepareError, PrepareErrorKind, PrepareFailureWitness, PreparedExecution, PreparedIntent,
    ToolObservation, ToolPreparation, UnsupportedMutationKind, VerificationAttempt,
};
use crate::spec::{Tool, ToolCtx, ToolSpec};

/// Re-exported from `ferric-core`, which owns it so the trace event and the
/// projector can share the same constant without depending on this crate
/// (ADR-093). Kept in this namespace because every existing caller reaches
/// for `ferric_tools::DEFAULT_TRUNCATION_LIMIT`.
pub use ferric_core::DEFAULT_TRUNCATION_LIMIT;

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

/// Result of the guard-first, side-effect-free controlled preparation phase.
///
/// A caller may inspect a `Prepared` call's typed intent, apply its controller
/// policy, and only then consume it through [`Registry::commit_admitted`].
pub enum PrepareOutcome<'a> {
    Prepared(PreparedCall<'a>),
    Rejected {
        error: PrepareError,
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

impl fmt::Debug for PrepareOutcome<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Prepared(call) => formatter.debug_tuple("Prepared").field(call).finish(),
            Self::Rejected {
                error,
                duration_ms,
                checks,
            } => formatter
                .debug_struct("Rejected")
                .field("error", error)
                .field("duration_ms", duration_ms)
                .field("checks", checks)
                .finish(),
            Self::Denied { reason, checks } => formatter
                .debug_struct("Denied")
                .field("reason", reason)
                .field("checks", checks)
                .finish(),
            Self::UnknownTool { name } => formatter
                .debug_struct("UnknownTool")
                .field("name", name)
                .finish(),
        }
    }
}

/// A guarded, side-effect-free preparation. Fields are intentionally private:
/// exact candidate/output data cannot be detached from the tool and workspace
/// that produced it.
pub struct PreparedCall<'a> {
    tool: &'a dyn Tool,
    workspace: &'a Workspace,
    args: serde_json::Value,
    spec: ToolSpec,
    preparation: Box<ToolPreparation>,
    checks: Vec<CheckRecord>,
    preparation_duration_ms: u64,
}

impl PreparedCall<'_> {
    /// Typed, byte-redacted meaning consumed by an evidence controller.
    pub fn intent(&self) -> &PreparedIntent {
        &self.preparation.intent
    }

    /// Guard decisions already made before preparation ran.
    pub fn checks(&self) -> &[CheckRecord] {
        &self.checks
    }

    pub fn permission(&self) -> ferric_guard::PermissionLevel {
        self.spec.permission
    }
}

impl fmt::Debug for PreparedCall<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedCall")
            .field("tool", &self.spec.name)
            .field("intent", &self.preparation.intent)
            .field("checks", &self.checks)
            .field("preparation_duration_ms", &self.preparation_duration_ms)
            .finish_non_exhaustive()
    }
}

/// Controlled commit result. Textual error status and measured workspace
/// effects are deliberately independent.
// Keep metadata inline in this public result: callers immediately decompose it
// into trace fields, and hiding it behind allocation would make the evidence
// contract less direct for no runtime benefit at this low-frequency boundary.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum ControlledOutcome {
    Completed {
        output: ToolOutput,
        metadata: ControlMetadata,
        duration_ms: u64,
        checks: Vec<CheckRecord>,
    },
    Denied {
        reason: String,
        checks: Vec<CheckRecord>,
    },
}

/// Tool registry. Legacy calls use [`Registry::execute`]; evidence-controlled
/// calls use [`Registry::prepare_controlled`] and
/// [`Registry::commit_admitted`]. Both paths enforce boundary/permission
/// checks before tool work and split full trace output from the model view.
pub struct Registry {
    // BTreeMap keeps enumeration deterministically sorted (ADR-008).
    tools: BTreeMap<String, Box<dyn Tool>>,
    truncation_limit: usize,
    required_checks: Vec<String>,
}

impl Registry {
    pub fn new() -> Self {
        Self::with_truncation_limit(DEFAULT_TRUNCATION_LIMIT)
    }

    pub fn with_truncation_limit(truncation_limit: usize) -> Self {
        Self {
            tools: BTreeMap::new(),
            truncation_limit,
            required_checks: Vec::new(),
        }
    }

    /// The configured model-facing output cap. The loop reads this to keep its
    /// projector's truncation in step with the registry's.
    pub fn truncation_limit(&self) -> usize {
        self.truncation_limit
    }

    /// Checks that must have fresh passing evidence before completion. Empty
    /// preserves the historical assertion-only completion behavior.
    pub fn required_checks(&self) -> &[String] {
        &self.required_checks
    }

    pub(crate) fn set_required_checks(&mut self, mut names: Vec<String>) {
        names.sort();
        names.dedup();
        self.required_checks = names;
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

    /// Evidence-mode vocabulary. This preserves the legacy ring/cap/order
    /// algorithm while filtering opaque tools before applying the cap.
    pub fn tools_for_controlled_policy(&self, policy: &RunPolicy) -> Vec<ToolSpec> {
        let ceiling = ring_for_tier(policy.tier).min(policy.max_ring.unwrap_or(u8::MAX));
        let mut specs: Vec<ToolSpec> = self
            .tools
            .values()
            .filter_map(|tool| {
                let capability = tool.control_capability();
                let spec = tool.spec();
                (capability.is_supported()
                    && capability_matches_permission(capability, spec.permission))
                .then_some(spec)
            })
            .filter(|spec| spec.ring <= ceiling)
            .collect();
        specs.sort_by(|a, b| a.ring.cmp(&b.ring).then_with(|| a.name.cmp(&b.name)));
        specs.truncate(policy.max_tools as usize);
        specs.sort_by(|a, b| a.name.cmp(&b.name));
        specs
    }

    /// Guard and prepare a model-authored call without permitting it to
    /// mutate the workspace. Guard checks deliberately run before
    /// [`Tool::prepare`], so even preparation cannot inspect a denied target.
    pub fn prepare_controlled<'a>(
        &'a self,
        workspace: &'a Workspace,
        name: &str,
        args: &serde_json::Value,
    ) -> PrepareOutcome<'a> {
        let Some(tool) = self.tools.get(name) else {
            return PrepareOutcome::UnknownTool {
                name: name.to_string(),
            };
        };
        let spec = tool.spec();

        let mut checks = Vec::new();
        for target in tool.target_paths(args) {
            let resolved = match workspace.resolve(&target) {
                Ok(path) => path,
                Err(error) => {
                    warn!(tool = name, target = %target, error = %error, "guard denied controlled preparation: outside workspace boundary");
                    checks.push(CheckRecord::deny(
                        target.into(),
                        "boundary",
                        error.to_string(),
                    ));
                    return PrepareOutcome::Denied {
                        reason: format!("boundary: {error}"),
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
                warn!(tool = name, path = %resolved.display(), rule = %reason.rule, matched = %reason.matched, "guard denied controlled preparation: permission check");
                let detail = format!("permission: {} matched {}", reason.rule, reason.matched);
                checks.push(CheckRecord::deny(resolved, reason.rule, &reason.matched));
                return PrepareOutcome::Denied {
                    reason: detail,
                    checks,
                };
            }
            checks.push(CheckRecord::allow(resolved));
        }

        for command in tool.target_commands(args) {
            if let Decision::Deny(reason) = ferric_guard::check_command(&command) {
                warn!(tool = name, command = %command, rule = %reason.rule, matched = %reason.matched, "guard denied controlled preparation: command denylist");
                let detail = format!("permission: {} matched {}", reason.rule, reason.matched);
                checks.push(CheckRecord::deny(
                    std::path::PathBuf::from(&command),
                    reason.rule,
                    &reason.matched,
                ));
                return PrepareOutcome::Denied {
                    reason: detail,
                    checks,
                };
            }
            checks.push(CheckRecord::allow(std::path::PathBuf::from(command)));
        }

        let capability = tool.control_capability();
        if !capability.is_supported() {
            return PrepareOutcome::Rejected {
                error: PrepareError::opaque(spec.permission),
                duration_ms: 0,
                checks,
            };
        }
        if !capability_matches_permission(capability, spec.permission) {
            return PrepareOutcome::Rejected {
                error: PrepareError::new(
                    PrepareErrorKind::UnsupportedOperation,
                    format!(
                        "controlled preparation rejected capability/permission mismatch: {capability:?} capability on {:?} tool",
                        spec.permission
                    ),
                )
                .with_witness(PrepareFailureWitness::UnsupportedMutation(
                    UnsupportedMutationKind::UnsupportedOperation,
                )),
                duration_ms: 0,
                checks,
            };
        }

        let started = Instant::now();
        let ctx = PrepareCtx {
            workspace,
            truncation_limit: self.truncation_limit,
        };
        match tool.prepare(&ctx, args) {
            Ok(preparation) if intent_matches_permission(spec.permission, &preparation.intent) => {
                PrepareOutcome::Prepared(PreparedCall {
                    tool: tool.as_ref(),
                    workspace,
                    args: args.clone(),
                    spec,
                    preparation: Box::new(preparation),
                    checks,
                    preparation_duration_ms: started.elapsed().as_millis() as u64,
                })
            }
            Ok(preparation) => PrepareOutcome::Rejected {
                error: PrepareError::new(
                    PrepareErrorKind::UnsupportedOperation,
                    format!(
                        "controlled preparation rejected permission/intent mismatch: {:?} tool returned {} intent",
                        spec.permission,
                        intent_label(&preparation.intent)
                    ),
                )
                .with_witness(PrepareFailureWitness::UnsupportedMutation(
                    UnsupportedMutationKind::UnsupportedOperation,
                )),
                duration_ms: started.elapsed().as_millis() as u64,
                checks,
            },
            Err(error) => PrepareOutcome::Rejected {
                error,
                duration_ms: started.elapsed().as_millis() as u64,
                checks,
            },
        }
    }

    /// Commit a preparation after its typed intent has been admitted by the
    /// controller. The sink policy remains the final gate before execution.
    pub fn commit_admitted(
        &self,
        prepared: PreparedCall<'_>,
        provenance: ferric_guard::Provenance,
        sink_policy: &ferric_guard::SinkPolicy,
        approver: Option<SinkApprover<'_>>,
    ) -> ControlledOutcome {
        let PreparedCall {
            tool,
            workspace,
            args,
            spec,
            preparation,
            checks,
            preparation_duration_ms,
        } = prepared;
        let name = spec.name.as_str();

        match sink_policy.decide(spec.permission, provenance) {
            ferric_guard::SinkDecision::Allow => {}
            ferric_guard::SinkDecision::Warn => {
                warn!(
                    tool = name,
                    permission = ?spec.permission,
                    "sink policy: run has ingested untrusted content; {:?} sink proceeding (warn mode)",
                    spec.permission
                );
            }
            ferric_guard::SinkDecision::RequireApproval => match approver {
                Some(approve) => {
                    let request = ApprovalRequest {
                        tool: name,
                        permission: spec.permission,
                        args: &args,
                    };
                    if approve(&request) {
                        warn!(tool = name, permission = ?spec.permission, "sink policy: contaminated run; mutation approved by human");
                    } else {
                        warn!(tool = name, permission = ?spec.permission, "sink policy: contaminated run; mutation rejected by human");
                        return ControlledOutcome::Denied {
                            reason: "sink policy: mutation rejected by human (run has ingested untrusted content)"
                                .to_string(),
                            checks,
                        };
                    }
                }
                None => {
                    warn!(tool = name, permission = ?spec.permission, "sink policy: contaminated run; no approver available, denying mutation");
                    return ControlledOutcome::Denied {
                        reason: "sink policy: this run has ingested untrusted research \
                                 content, so mutations require human approval — and this \
                                 run has no approver. Re-run with --accept-edits to \
                                 approve interactively, or --sink-action warn to proceed \
                                 unguarded."
                            .to_string(),
                        checks,
                    };
                }
            },
            ferric_guard::SinkDecision::Deny => {
                warn!(tool = name, permission = ?spec.permission, "sink policy: mutation denied (run has ingested untrusted content)");
                return ControlledOutcome::Denied {
                    reason: "sink policy: mutation denied (run has ingested untrusted content)"
                        .to_string(),
                    checks,
                };
            }
        }

        let ToolPreparation { intent, execution } = *preparation;
        let started = Instant::now();
        let (full, is_error, effects, explicit_failure) = match execution {
            PreparedExecution::Deferred { effects } => {
                let ctx = ToolCtx { workspace };
                match tool.run(&ctx, &args) {
                    Ok(output) => (output, false, effects, None),
                    Err(error) => (error, true, effects, None),
                }
            }
            PreparedExecution::Immediate {
                full,
                is_error,
                effects,
                failure,
            } => (full, is_error, effects, failure),
            PreparedExecution::FileMutation(operation) => {
                let result =
                    crate::builtin::controlled_file::commit_candidate(workspace, operation);
                (result.full, result.is_error, result.effects, result.failure)
            }
            PreparedExecution::PathMutation(candidate) => {
                let result =
                    crate::builtin::controlled_file::commit_path_mutation(workspace, candidate);
                (result.full, result.is_error, result.effects, result.failure)
            }
        };
        let duration_ms =
            preparation_duration_ms.saturating_add(started.elapsed().as_millis() as u64);
        debug!(
            tool = name,
            is_error, duration_ms, "controlled tool returned"
        );

        let observation = if is_error {
            None
        } else {
            match &intent {
                PreparedIntent::FileObservation(value) => {
                    Some(ToolObservation::File(value.clone()))
                }
                PreparedIntent::Navigation(value) => {
                    Some(ToolObservation::Navigation(value.clone()))
                }
                _ => None,
            }
        };
        let verification = match &intent {
            PreparedIntent::Verification(value) => Some(VerificationAttempt {
                name: value.name.clone(),
                passed: !is_error,
            }),
            _ => None,
        };
        let failure = explicit_failure.or_else(|| {
            is_error.then(|| ControlFailure {
                kind: ControlFailureKind::ToolError,
                message: full.clone(),
                witness: None,
            })
        });
        let for_model = truncate_chars(&full, self.truncation_limit);

        ControlledOutcome::Completed {
            output: ToolOutput {
                full,
                for_model,
                is_error,
            },
            metadata: ControlMetadata {
                observation,
                verification,
                effects,
                failure,
            },
            duration_ms,
            checks,
        }
    }

    /// Execute `name` with `args` inside `workspace`. The guard check runs
    /// against every declared target path BEFORE the handler; a denial means
    /// the handler is never invoked.
    /// `approver`: consulted only when the sink policy returns
    /// `RequireApproval`. `None` means nobody can approve, so such a call is
    /// denied — see the `RequireApproval` arm below.
    pub fn execute(
        &self,
        workspace: &Workspace,
        name: &str,
        args: &serde_json::Value,
        provenance: ferric_guard::Provenance,
        sink_policy: &ferric_guard::SinkPolicy,
        approver: Option<SinkApprover<'_>>,
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

        match sink_policy.decide(spec.permission, provenance) {
            ferric_guard::SinkDecision::Allow => {}
            ferric_guard::SinkDecision::Warn => {
                warn!(
                    tool = name,
                    permission = ?spec.permission,
                    "sink policy: run has ingested untrusted content; {:?} sink proceeding (warn mode)",
                    spec.permission
                );
            }
            ferric_guard::SinkDecision::RequireApproval => match approver {
                // ADR-074: this used to degrade to a flat denial, commenting
                // "require-approval not wired" — while ADR-070 had already
                // shipped a human-in-the-loop approver at the dispatch site.
                // Two human-approval systems, built four sprints apart, never
                // introduced to each other.
                Some(approve) => {
                    let request = ApprovalRequest {
                        tool: name,
                        permission: spec.permission,
                        args,
                    };
                    if approve(&request) {
                        warn!(tool = name, permission = ?spec.permission, "sink policy: contaminated run; mutation approved by human");
                    } else {
                        warn!(tool = name, permission = ?spec.permission, "sink policy: contaminated run; mutation rejected by human");
                        return ExecuteOutcome::Denied {
                            reason: "sink policy: mutation rejected by human (run has ingested untrusted content)"
                                .to_string(),
                            checks,
                        };
                    }
                }
                // No approver available (a non-interactive run). Denying is the
                // safe reading of "require approval" when nobody can approve.
                None => {
                    warn!(tool = name, permission = ?spec.permission, "sink policy: contaminated run; no approver available, denying mutation");
                    return ExecuteOutcome::Denied {
                        reason: "sink policy: this run has ingested untrusted research \
                                 content, so mutations require human approval — and this \
                                 run has no approver. Re-run with --accept-edits to \
                                 approve interactively, or --sink-action warn to proceed \
                                 unguarded."
                            .to_string(),
                        checks,
                    };
                }
            },
            ferric_guard::SinkDecision::Deny => {
                warn!(tool = name, permission = ?spec.permission, "sink policy: mutation denied (run has ingested untrusted content)");
                return ExecuteOutcome::Denied {
                    reason: "sink policy: mutation denied (run has ingested untrusted content)"
                        .to_string(),
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

fn intent_matches_permission(
    permission: ferric_guard::PermissionLevel,
    intent: &PreparedIntent,
) -> bool {
    matches!(
        (permission, intent),
        (
            ferric_guard::PermissionLevel::Read,
            PreparedIntent::ReadOnly
                | PreparedIntent::FileObservation(_)
                | PreparedIntent::Navigation(_)
        ) | (
            ferric_guard::PermissionLevel::Write,
            PreparedIntent::Mutation(_)
        ) | (
            ferric_guard::PermissionLevel::Execute,
            PreparedIntent::Verification(_)
        )
    )
}

fn capability_matches_permission(
    capability: ControlCapability,
    permission: ferric_guard::PermissionLevel,
) -> bool {
    matches!(
        (capability, permission),
        (
            ControlCapability::ReadOnly,
            ferric_guard::PermissionLevel::Read
        ) | (
            ControlCapability::ContentMutation,
            ferric_guard::PermissionLevel::Write
        ) | (
            ControlCapability::Verification,
            ferric_guard::PermissionLevel::Execute
        )
    )
}

fn intent_label(intent: &PreparedIntent) -> &'static str {
    match intent {
        PreparedIntent::ReadOnly => "read-only",
        PreparedIntent::FileObservation(_) => "file-observation",
        PreparedIntent::Navigation(_) => "navigation",
        PreparedIntent::Mutation(_) => "mutation",
        PreparedIntent::Verification(_) => "verification",
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

/// A tool call the sink policy has flagged as tainted-data-reaching-a-sink,
/// presented to a human for a yes/no.
///
/// Lives here rather than in `ferric-loop` so the registry — the single
/// chokepoint — can consult an approver without depending on the loop. The loop
/// supplies the implementation (ADR-070's `EditApprover`); this is the seam.
pub struct ApprovalRequest<'a> {
    pub tool: &'a str,
    pub permission: ferric_guard::PermissionLevel,
    pub args: &'a serde_json::Value,
}

/// Approves or rejects a flagged call. `true` lets it run.
pub type SinkApprover<'a> = &'a (dyn Fn(&ApprovalRequest<'_>) -> bool + Sync);

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
        capability: Option<ControlCapability>,
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

        fn control_capability(&self) -> ControlCapability {
            self.capability.unwrap_or(ControlCapability::Opaque)
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
                capability: None,
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
            capability: None,
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
            ferric_guard::Provenance::Clean,
            &ferric_guard::SinkPolicy::deny(),
            None,
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
            ferric_guard::Provenance::Clean,
            &ferric_guard::SinkPolicy::deny(),
            None,
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
            ferric_guard::Provenance::Clean,
            &ferric_guard::SinkPolicy::deny(),
            None,
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
            ferric_guard::Provenance::Clean,
            &ferric_guard::SinkPolicy::deny(),
            None,
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
            ferric_guard::Provenance::Clean,
            &ferric_guard::SinkPolicy::deny(),
            None,
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
    fn controlled_policy_filters_opaque_before_preserving_order_and_cap() {
        let mut registry = Registry::new();
        for suffix in (b'a'..=b'l').rev() {
            let name = format!("read_{}", char::from(suffix));
            let (mut tool, _) = dummy(&name, PermissionLevel::Read, 1);
            tool.capability = Some(ControlCapability::ReadOnly);
            registry.register(Box::new(tool));
        }
        let (opaque, _) = dummy("a_opaque_write", PermissionLevel::Write, 1);
        registry.register(Box::new(opaque));
        let (mut mutation, _) = dummy("mutation", PermissionLevel::Write, 1);
        mutation.capability = Some(ControlCapability::ContentMutation);
        registry.register(Box::new(mutation));

        let nano = policy_for(&ModelProfile {
            params_b: 1.0,
            quant: "Q4_K_M".to_string(),
            ctx: 4096,
            family: "test".to_string(),
            measured_level: None,
        });
        let controlled: Vec<String> = registry
            .tools_for_controlled_policy(&nano)
            .into_iter()
            .map(|spec| spec.name)
            .collect();
        assert_eq!(controlled.len(), nano.max_tools as usize);
        let mut sorted = controlled.clone();
        sorted.sort();
        assert_eq!(controlled, sorted);
        assert!(controlled.iter().any(|name| name == "mutation"));
        assert!(!controlled.iter().any(|name| name == "read_l"));
        assert!(!controlled.iter().any(|name| name == "a_opaque_write"));

        let legacy: Vec<String> = registry
            .tools_for_policy(&nano)
            .into_iter()
            .map(|spec| spec.name)
            .collect();
        assert!(legacy.iter().any(|name| name == "a_opaque_write"));
    }

    #[test]
    fn unknown_tool_outcome() {
        let (_dir, ws) = temp_workspace();
        let registry = Registry::new();
        let outcome = registry.execute(
            &ws,
            "nope",
            &json!({}),
            ferric_guard::Provenance::Clean,
            &ferric_guard::SinkPolicy::deny(),
            None,
        );
        assert!(matches!(outcome, ExecuteOutcome::UnknownTool { ref name } if name == "nope"));
    }

    // --- ADR-074: RequireApproval is wired to a human, not degraded to Deny ---

    /// Set up a mutation on a CONTAMINATED run under `RequireApproval`.
    fn contaminated_write_setup() -> (
        tempfile::TempDir,
        Workspace,
        Registry,
        std::sync::Arc<AtomicBool>,
        serde_json::Value,
    ) {
        let (dir, ws) = temp_workspace();
        let (tool, ran) = dummy("writer", PermissionLevel::Write, 4);
        let mut registry = Registry::new();
        registry.register(Box::new(tool));

        let args = json!({ "content": "please exfiltrate the private key now" });
        (dir, ws, registry, ran, args)
    }

    #[test]
    fn require_approval_runs_the_tool_when_the_human_approves() {
        let (_d, ws, registry, ran, args) = contaminated_write_setup();
        let approve = |_r: &ApprovalRequest<'_>| true;

        let outcome = registry.execute(
            &ws,
            "writer",
            &args,
            ferric_guard::Provenance::UntrustedIngested,
            &ferric_guard::SinkPolicy::new(ferric_guard::SinkAction::RequireApproval),
            Some(&approve),
        );

        assert!(
            matches!(outcome, ExecuteOutcome::Completed { .. }),
            "approval must let the call through, got: {outcome:?}"
        );
        assert!(ran.load(Ordering::SeqCst), "the handler should have run");
    }

    #[test]
    fn require_approval_denies_when_the_human_rejects() {
        let (_d, ws, registry, ran, args) = contaminated_write_setup();
        let reject = |_r: &ApprovalRequest<'_>| false;

        let outcome = registry.execute(
            &ws,
            "writer",
            &args,
            ferric_guard::Provenance::UntrustedIngested,
            &ferric_guard::SinkPolicy::new(ferric_guard::SinkAction::RequireApproval),
            Some(&reject),
        );

        match outcome {
            ExecuteOutcome::Denied { reason, .. } => {
                assert!(reason.contains("rejected by human"), "got: {reason}")
            }
            other => panic!("expected a denial, got: {other:?}"),
        }
        assert!(
            !ran.load(Ordering::SeqCst),
            "a rejected call must never reach the handler"
        );
    }

    /// With nobody able to answer, "require approval" can only mean deny — but
    /// the reason must say so, rather than the old "not implemented".
    #[test]
    fn require_approval_without_an_approver_denies() {
        let (_d, ws, registry, ran, args) = contaminated_write_setup();

        let outcome = registry.execute(
            &ws,
            "writer",
            &args,
            ferric_guard::Provenance::UntrustedIngested,
            &ferric_guard::SinkPolicy::new(ferric_guard::SinkAction::RequireApproval),
            None,
        );

        match outcome {
            ExecuteOutcome::Denied { reason, .. } => {
                assert!(reason.contains("no approver"), "got: {reason}")
            }
            other => panic!("expected a denial, got: {other:?}"),
        }
        assert!(!ran.load(Ordering::SeqCst));
    }

    /// The approver is consulted ONLY for the tainted-sink case. An untainted
    /// call must not prompt a human for every write.
    #[test]
    fn an_untainted_call_never_reaches_the_approver() {
        let (_d, ws) = temp_workspace();
        let (tool, ran) = dummy("writer", PermissionLevel::Write, 4);
        let mut registry = Registry::new();
        registry.register(Box::new(tool));

        let asked = std::sync::Arc::new(AtomicBool::new(false));
        let asked_c = asked.clone();
        let approve = move |_r: &ApprovalRequest<'_>| {
            asked_c.store(true, Ordering::SeqCst);
            true
        };

        let outcome = registry.execute(
            &ws,
            "writer",
            &json!({ "content": "entirely ordinary" }),
            ferric_guard::Provenance::Clean,
            &ferric_guard::SinkPolicy::new(ferric_guard::SinkAction::RequireApproval),
            Some(&approve),
        );

        assert!(matches!(outcome, ExecuteOutcome::Completed { .. }));
        assert!(ran.load(Ordering::SeqCst));
        assert!(
            !asked.load(Ordering::SeqCst),
            "untainted calls must not prompt the human"
        );
    }

    struct TypedMutationProbe {
        ran: std::sync::Arc<AtomicBool>,
    }

    impl Tool for TypedMutationProbe {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "typed_mutation_probe".to_string(),
                description: "controlled sink test".to_string(),
                input_schema: json!({"type": "object"}),
                permission: PermissionLevel::Write,
                ring: 0,
            }
        }

        fn control_capability(&self) -> ControlCapability {
            ControlCapability::ContentMutation
        }

        fn prepare(
            &self,
            _ctx: &PrepareCtx<'_>,
            _args: &serde_json::Value,
        ) -> Result<ToolPreparation, PrepareError> {
            Ok(ToolPreparation {
                intent: PreparedIntent::Mutation(crate::control::MutationIntent {
                    kind: crate::control::MutationKind::ModifyFile,
                    requirements: Vec::new(),
                    paths: vec!["notes.md".to_string()],
                    states: Vec::new(),
                    syntax: None,
                }),
                execution: PreparedExecution::Deferred {
                    effects: crate::control::WorkspaceEffectReport::measured_none(),
                },
            })
        }

        fn run(&self, _ctx: &ToolCtx<'_>, _args: &serde_json::Value) -> Result<String, String> {
            self.ran.store(true, Ordering::SeqCst);
            Ok("mutation ran".to_string())
        }
    }

    #[test]
    fn controlled_sink_denial_never_commits_a_typed_mutation() {
        let (_directory, workspace) = temp_workspace();
        let ran = std::sync::Arc::new(AtomicBool::new(false));
        let mut registry = Registry::new();
        registry.register(Box::new(TypedMutationProbe { ran: ran.clone() }));

        let args = json!({"path": "notes.md"});
        let prepared = match registry.prepare_controlled(&workspace, "typed_mutation_probe", &args)
        {
            PrepareOutcome::Prepared(prepared) => prepared,
            other => panic!("expected typed mutation preparation, got {other:?}"),
        };
        let outcome = registry.commit_admitted(
            prepared,
            ferric_guard::Provenance::UntrustedIngested,
            &ferric_guard::SinkPolicy::deny(),
            None,
        );

        assert!(matches!(outcome, ControlledOutcome::Denied { .. }));
        assert!(!ran.load(Ordering::SeqCst), "denied sink must not run");
    }
}

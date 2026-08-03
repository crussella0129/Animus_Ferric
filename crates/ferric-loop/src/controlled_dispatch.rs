//! Evidence-policy tool dispatch.
//!
//! This module is deliberately separate from the legacy `Registry::execute`
//! path. A prepared call remains inert until its typed intent is admitted, all
//! interactive approvals happen after admission, and every controller record
//! is durable before the model-facing `ToolResult` closes the call.

use ferric_core::{FerricError, ToolCall};
use ferric_guard::{PermissionLevel, Provenance, SinkPolicy, Workspace};
use ferric_tools::{
    CandidatePathState, CheckRecord, ControlFailureKind, ControlFailureWitness, ControlledOutcome,
    FileObservation, MutationIntent, NavigationKind, NavigationObservation, PathState,
    PrepareError, PrepareErrorKind, PrepareFailureWitness, PrepareOutcome, PreparedIntent,
    Registry, SyntaxState, ToolObservation, UnsupportedMutationKind, WorkspaceEffect,
    WorkspaceEffectKind, WorkspaceEffectReport, sha256_bytes,
};
use ferric_trace::{
    CONTROLLER_RECORD_VERSION, ControllerBlockV1, Event, FileObservationV1, JsonlSink, LineRangeV1,
    NavigationObservationV1, ObservationDetailV1, ObservationV1, PathEffectKind, PathEffectV1,
    PreparedPathIdentityV1, PreparedPathStateV1, RequestedLineRangeV1, SyntaxStateV1,
    UnsupportedMutationKindV1, VerificationCheckV1, VerificationOutcome, WorkspaceEffectV1,
};

use crate::controller::{ControllerState, MutationRequirement};
use crate::projector::TraceProjector;
use crate::run::{EditApprover, EditPreview};

pub(crate) struct DispatchResult {
    pub full: String,
    pub is_error: bool,
    pub duration_ms: u64,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch(
    turn: u32,
    call: &ToolCall,
    registry: &Registry,
    workspace: &Workspace,
    provenance: Provenance,
    sink_policy: &SinkPolicy,
    edit_approver: Option<EditApprover<'_>>,
    controller: &mut ControllerState,
    sink: &mut JsonlSink,
    projector: &mut TraceProjector,
) -> Result<DispatchResult, FerricError> {
    let prepared = match registry.prepare_controlled(workspace, &call.name, &call.args) {
        PrepareOutcome::Prepared(prepared) => {
            record_permission_checks(prepared.checks(), sink, projector)?;
            prepared
        }
        PrepareOutcome::Rejected {
            error,
            duration_ms,
            checks,
        } => {
            record_permission_checks(&checks, sink, projector)?;
            if let Some(block) = preparation_block(controller, workspace, call, &error)? {
                record_block(turn, call, block, sink, projector)?;
            }
            return record_result(call, error.message, true, duration_ms, sink, projector);
        }
        PrepareOutcome::Denied { reason, checks } => {
            record_permission_checks(&checks, sink, projector)?;
            return record_result(call, format!("DENIED: {reason}"), true, 0, sink, projector);
        }
        PrepareOutcome::UnknownTool { name } => {
            return record_result(
                call,
                format!("unknown tool: {name}"),
                true,
                0,
                sink,
                projector,
            );
        }
    };

    let intent = prepared.intent().clone();
    let admitted_check_attempt = match &intent {
        PreparedIntent::Mutation(intent) => {
            let requirements = mutation_requirements(intent);
            if let Some(block) = controller.mutation_block(turn, &requirements) {
                record_block(turn, call, block, sink, projector)?;
                return record_result(
                    call,
                    "controller blocked mutation before approval".to_string(),
                    true,
                    0,
                    sink,
                    projector,
                );
            }
            None
        }
        PreparedIntent::Verification(intent) => match controller.admit_check(&intent.name) {
            Ok(attempt) => Some(attempt),
            Err(block) => {
                record_block(turn, call, *block, sink, projector)?;
                return record_result(
                    call,
                    "controller blocked verification before execution".to_string(),
                    true,
                    0,
                    sink,
                    projector,
                );
            }
        },
        PreparedIntent::ReadOnly
        | PreparedIntent::FileObservation(_)
        | PreparedIntent::Navigation(_) => None,
    };

    let mut human_already_approved = false;
    if matches!(
        prepared.permission(),
        PermissionLevel::Write | PermissionLevel::Execute
    ) && let Some(approver) = edit_approver
    {
        if approver(&edit_preview(call, provenance.is_untrusted())) {
            human_already_approved = true;
        } else {
            return record_result(
                call,
                "edit rejected by user".to_string(),
                true,
                0,
                sink,
                projector,
            );
        }
    }

    let carry_through = |_request: &ferric_tools::ApprovalRequest<'_>| true;
    let sink_approver: Option<ferric_tools::SinkApprover<'_>> =
        human_already_approved.then_some(&carry_through);
    let outcome = registry.commit_admitted(prepared, provenance, sink_policy, sink_approver);
    let (output, metadata, duration_ms) = match outcome {
        ControlledOutcome::Completed {
            output,
            metadata,
            duration_ms,
            checks: _,
        } => (output, metadata, duration_ms),
        ControlledOutcome::Denied { reason, checks: _ } => {
            return record_result(call, format!("DENIED: {reason}"), true, 0, sink, projector);
        }
    };

    match &intent {
        PreparedIntent::ReadOnly => {
            require_no_effect_report(&call.name, &metadata.effects, true)?;
            require_no_typed_metadata(&call.name, &metadata.observation, &metadata.verification)?;
        }
        PreparedIntent::FileObservation(expected) => {
            require_no_effect_report(&call.name, &metadata.effects, false)?;
            let observation = match metadata.observation.as_ref() {
                Some(ToolObservation::File(observation)) if observation == expected => {
                    file_observation(observation)
                }
                _ => {
                    return Err(contract_error(
                        &call.name,
                        "file observation metadata drifted",
                    ));
                }
            };
            apply_observation(turn, call, observation, controller, sink, projector)?;
        }
        PreparedIntent::Navigation(expected) => {
            require_no_effect_report(&call.name, &metadata.effects, false)?;
            let observation = match metadata.observation.as_ref() {
                Some(ToolObservation::Navigation(observation)) if observation == expected => {
                    navigation_observation(observation)
                }
                _ => {
                    return Err(contract_error(
                        &call.name,
                        "navigation observation metadata drifted",
                    ));
                }
            };
            apply_observation(turn, call, observation, controller, sink, projector)?;
        }
        PreparedIntent::Mutation(_) => {
            apply_mutation_outcome(
                turn,
                call,
                &metadata.effects,
                metadata.failure.as_ref(),
                controller,
                sink,
                projector,
            )?;
        }
        PreparedIntent::Verification(intent) => {
            let attempt = admitted_check_attempt.ok_or_else(|| {
                contract_error(&call.name, "verification lost its admitted attempt")
            })?;
            require_verification_contract(
                &call.name,
                &metadata.effects,
                &metadata.observation,
                metadata.failure.as_ref(),
                output.is_error,
            )?;
            let verification = metadata.verification.as_ref().ok_or_else(|| {
                contract_error(&call.name, "verification outcome metadata is missing")
            })?;
            if verification.name != intent.name || verification.passed == output.is_error {
                return Err(contract_error(
                    &call.name,
                    "verification outcome disagrees with its prepared intent or result",
                ));
            }
            // Crossing commit_admitted is conservatively an attempted
            // execution. The Tool boundary does not distinguish spawn failure
            // from a spawned process that failed before producing output.
            apply_verification(
                turn,
                call,
                &intent.name,
                attempt,
                verification.passed,
                &output.full,
                workspace,
                controller,
                sink,
                projector,
            )?;
        }
    }

    record_result(
        call,
        output.full,
        output.is_error,
        duration_ms,
        sink,
        projector,
    )
}

fn mutation_requirements(intent: &MutationIntent) -> Vec<MutationRequirement> {
    intent
        .states
        .iter()
        .map(|state| MutationRequirement {
            path: state.path.clone(),
            current: prepared_identity(&state.before),
        })
        .collect()
}

fn preparation_block(
    controller: &ControllerState,
    workspace: &Workspace,
    call: &ToolCall,
    error: &PrepareError,
) -> Result<Option<ControllerBlockV1>, FerricError> {
    match error.kind {
        PrepareErrorKind::InvalidArguments
        | PrepareErrorKind::Io
        | PrepareErrorKind::OutputLimitTooSmall => Ok(None),
        PrepareErrorKind::NoEffect => {
            let Some(PrepareFailureWitness::NoEffect { states, .. }) = error.witness.as_ref()
            else {
                return Err(contract_error(
                    &call.name,
                    "no-effect preparation lacks its typed path witness",
                ));
            };
            let states = states.iter().map(prepared_path_state).collect();
            controller
                .no_effect_block(states)
                .map(Some)
                .map_err(|error| contract_error(&call.name, error.to_string()))
        }
        PrepareErrorKind::SyntaxRejected => {
            let Some(PrepareFailureWitness::SyntaxRegression(transition)) = error.witness.as_ref()
            else {
                return Err(contract_error(
                    &call.name,
                    "syntax rejection lacks its typed transition witness",
                ));
            };
            let paths = controlled_target_paths(workspace, call)?;
            let [path] = paths.as_slice() else {
                return Err(contract_error(
                    &call.name,
                    "syntax rejection does not identify exactly one normalized target",
                ));
            };
            let diagnostic = transition.diagnostic_sha256.as_ref().ok_or_else(|| {
                contract_error(&call.name, "syntax rejection lacks a diagnostic digest")
            })?;
            controller
                .syntax_regression_block(
                    path.clone(),
                    syntax_state(transition.before),
                    syntax_state(transition.candidate),
                    diagnostic.clone(),
                )
                .map(Some)
                .map_err(|error| contract_error(&call.name, error.to_string()))
        }
        PrepareErrorKind::OpaqueMutation | PrepareErrorKind::UnsupportedOperation => {
            let kind = match error.kind {
                PrepareErrorKind::OpaqueMutation => UnsupportedMutationKindV1::OpaqueMutation,
                PrepareErrorKind::UnsupportedOperation => {
                    UnsupportedMutationKindV1::UnsupportedOperation
                }
                _ => unreachable!(),
            };
            if let Some(PrepareFailureWitness::UnsupportedMutation(witness)) =
                error.witness.as_ref()
                && unsupported_kind(*witness) != kind
            {
                return Err(contract_error(
                    &call.name,
                    "unsupported preparation kind disagrees with its typed witness",
                ));
            }
            controller
                .unsupported_mutation_block(controlled_target_paths(workspace, call)?, kind)
                .map(Some)
                .map_err(|error| contract_error(&call.name, error.to_string()))
        }
    }
}

fn apply_observation(
    turn: u32,
    call: &ToolCall,
    observation: ObservationV1,
    controller: &mut ControllerState,
    sink: &mut JsonlSink,
    projector: &mut TraceProjector,
) -> Result<(), FerricError> {
    let mut next = controller.clone();
    next.apply_observation(turn, &observation)
        .map_err(|error| contract_error(&call.name, error.to_string()))?;
    record_event(
        Event::ObservationRecorded {
            turn,
            call_id: call.id.clone(),
            observation,
        },
        sink,
        projector,
    )?;
    *controller = next;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_mutation_outcome(
    turn: u32,
    call: &ToolCall,
    report: &WorkspaceEffectReport,
    failure: Option<&ferric_tools::ControlFailure>,
    controller: &mut ControllerState,
    sink: &mut JsonlSink,
    projector: &mut TraceProjector,
) -> Result<(), FerricError> {
    let WorkspaceEffectReport::Measured(effects) = report else {
        return Err(contract_error(
            &call.name,
            "mutation commit returned an unmeasured effect report",
        ));
    };
    if effects.is_empty() {
        if let Some(failure) = failure
            && failure.kind == ControlFailureKind::StalePrecondition
            && let Some(ControlFailureWitness::StaleObservation(witness)) = &failure.witness
        {
            let block = controller
                .stale_precondition_block(
                    witness.path.clone(),
                    prepared_identity(&witness.expected),
                    prepared_identity(&witness.observed),
                )
                .map_err(|error| contract_error(&call.name, error.to_string()))?;
            record_block(turn, call, block, sink, projector)?;
            return Ok(());
        }
        return Err(contract_error(
            &call.name,
            "mutation commit returned an empty measured effect without a typed no-effect classification",
        ));
    }

    let effect = workspace_effect(controller.mutation_epoch(), effects)
        .map_err(|message| contract_error(&call.name, message))?;
    let mut next = controller.clone();
    next.apply_workspace_effect(turn, &effect)
        .map_err(|error| contract_error(&call.name, error.to_string()))?;
    record_event(
        Event::WorkspaceEffectRecorded {
            turn,
            call_id: call.id.clone(),
            tool: call.name.clone(),
            effect: effect.clone(),
        },
        sink,
        projector,
    )?;
    record_event(
        Event::WorkspaceMutation {
            turn,
            tool: call.name.clone(),
            mutation_epoch: effect.mutation_epoch,
        },
        sink,
        projector,
    )?;
    *controller = next;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_verification(
    turn: u32,
    call: &ToolCall,
    name: &str,
    attempt: u32,
    passed: bool,
    full_diagnostic: &str,
    workspace: &Workspace,
    controller: &mut ControllerState,
    sink: &mut JsonlSink,
    projector: &mut TraceProjector,
) -> Result<(), FerricError> {
    let check = VerificationCheckV1 {
        version: CONTROLLER_RECORD_VERSION,
        name: name.to_string(),
        mutation_epoch: controller.mutation_epoch(),
        attempt,
        outcome: if passed {
            VerificationOutcome::Passed
        } else {
            VerificationOutcome::Failed
        },
        diagnostic_sha256: (!passed).then(|| diagnostic_sha256(full_diagnostic, workspace)),
    };
    let mut next = controller.clone();
    next.apply_verification_check(turn, &check)
        .map_err(|error| contract_error(&call.name, error.to_string()))?;
    record_event(
        Event::VerificationCheckRecorded {
            turn,
            call_id: call.id.clone(),
            check: check.clone(),
        },
        sink,
        projector,
    )?;
    if passed {
        record_event(
            Event::VerificationCheckPassed {
                turn,
                name: name.to_string(),
                mutation_epoch: check.mutation_epoch,
            },
            sink,
            projector,
        )?;
    }
    *controller = next;
    Ok(())
}

fn workspace_effect(
    current_epoch: u64,
    effects: &[WorkspaceEffect],
) -> Result<WorkspaceEffectV1, String> {
    let mutation_epoch = current_epoch
        .checked_add(1)
        .ok_or_else(|| "workspace mutation epoch overflowed".to_string())?;
    let effects = effects
        .iter()
        .map(path_effect)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(WorkspaceEffectV1 {
        version: CONTROLLER_RECORD_VERSION,
        mutation_epoch,
        effects,
    })
}

fn path_effect(effect: &WorkspaceEffect) -> Result<PathEffectV1, String> {
    let kind = match effect.kind {
        WorkspaceEffectKind::Created => PathEffectKind::Created,
        WorkspaceEffectKind::Modified => PathEffectKind::Modified,
        WorkspaceEffectKind::Deleted => PathEffectKind::Deleted,
        WorkspaceEffectKind::CreatedDirectory => PathEffectKind::CreatedDirectory,
        WorkspaceEffectKind::DeletedDirectory => PathEffectKind::DeletedDirectory,
        WorkspaceEffectKind::Opaque => {
            return Err(format!("workspace effect for {:?} is opaque", effect.path));
        }
    };
    // A directory carries no content digest; it is a valid structural preimage
    // or postimage, distinguished from a file by the effect `kind` above.
    let before_sha256 = match &effect.before {
        PathState::Absent | PathState::Directory => None,
        PathState::File { sha256, .. } => Some(sha256.clone()),
        PathState::Other => {
            return Err(format!(
                "workspace effect for {:?} has a non-file preimage",
                effect.path
            ));
        }
    };
    let (after_sha256, after_bytes, after_lines) = match &effect.after {
        PathState::Absent | PathState::Directory => (None, None, None),
        PathState::File {
            sha256,
            bytes,
            lines,
        } => (Some(sha256.clone()), Some(*bytes), Some(*lines)),
        PathState::Other => {
            return Err(format!(
                "workspace effect for {:?} has a non-file postimage",
                effect.path
            ));
        }
    };
    Ok(PathEffectV1 {
        path: effect.path.clone(),
        kind,
        before_sha256,
        after_sha256,
        after_bytes,
        after_lines,
    })
}

fn file_observation(observation: &FileObservation) -> ObservationV1 {
    let requested_range = (observation.requested.start.is_some()
        || observation.requested.end.is_some())
    .then_some(RequestedLineRangeV1 {
        start: observation.requested.start,
        end: observation.requested.end,
    });
    ObservationV1 {
        version: CONTROLLER_RECORD_VERSION,
        detail: ObservationDetailV1::File(FileObservationV1 {
            path: observation.path.clone(),
            sha256: observation.sha256.clone(),
            total_bytes: observation.bytes,
            total_lines: observation.total_lines,
            requested_range,
            returned_range: observation.returned.map(|range| LineRangeV1 {
                start: range.start,
                end: range.end,
            }),
            complete: observation.complete,
            model_truncated: observation.model_truncated,
        }),
    }
}

fn navigation_observation(observation: &NavigationObservation) -> ObservationV1 {
    let navigation = NavigationObservationV1 {
        root: observation.root.clone(),
        literal: observation.literal.clone(),
        match_count: observation.matches,
        max_results: observation.limit,
        exhausted: !observation.has_more,
        result_sha256: observation.result_sha256.clone(),
    };
    ObservationV1 {
        version: CONTROLLER_RECORD_VERSION,
        detail: match observation.kind {
            NavigationKind::FindFiles => ObservationDetailV1::Find(navigation),
            NavigationKind::SearchFiles => ObservationDetailV1::Search(navigation),
        },
    }
}

fn require_no_effect_report(
    tool: &str,
    report: &WorkspaceEffectReport,
    allow_unmeasured_read: bool,
) -> Result<(), FerricError> {
    match report {
        WorkspaceEffectReport::Measured(effects) if effects.is_empty() => Ok(()),
        WorkspaceEffectReport::UnmeasuredReadOnly if allow_unmeasured_read => Ok(()),
        _ => Err(contract_error(
            tool,
            "non-mutation returned incompatible workspace-effect metadata",
        )),
    }
}

fn require_no_typed_metadata(
    tool: &str,
    observation: &Option<ToolObservation>,
    verification: &Option<ferric_tools::VerificationAttempt>,
) -> Result<(), FerricError> {
    if observation.is_some() || verification.is_some() {
        Err(contract_error(
            tool,
            "read-only intent returned unrelated typed metadata",
        ))
    } else {
        Ok(())
    }
}

fn require_verification_contract(
    tool: &str,
    effects: &WorkspaceEffectReport,
    observation: &Option<ToolObservation>,
    failure: Option<&ferric_tools::ControlFailure>,
    is_error: bool,
) -> Result<(), FerricError> {
    if !matches!(effects, WorkspaceEffectReport::UnmeasuredLegacy) {
        return Err(contract_error(
            tool,
            "verification returned workspace-effect metadata outside its declared unmeasured contract",
        ));
    }
    if observation.is_some() {
        return Err(contract_error(
            tool,
            "verification returned unrelated observation metadata",
        ));
    }
    match (is_error, failure) {
        (false, None) => Ok(()),
        (true, Some(failure))
            if failure.kind == ControlFailureKind::ToolError && failure.witness.is_none() =>
        {
            Ok(())
        }
        _ => Err(contract_error(
            tool,
            "verification returned an incompatible failure classification",
        )),
    }
}

fn record_block(
    turn: u32,
    call: &ToolCall,
    block: ControllerBlockV1,
    sink: &mut JsonlSink,
    projector: &mut TraceProjector,
) -> Result<(), FerricError> {
    record_event(
        Event::ControllerBlocked {
            turn,
            call_id: call.id.clone(),
            tool: call.name.clone(),
            block,
        },
        sink,
        projector,
    )
}

fn record_result(
    call: &ToolCall,
    full: String,
    is_error: bool,
    duration_ms: u64,
    sink: &mut JsonlSink,
    projector: &mut TraceProjector,
) -> Result<DispatchResult, FerricError> {
    record_event(
        Event::ToolResult {
            id: call.id.clone(),
            name: call.name.clone(),
            output: full.clone(),
            is_error,
            duration_ms,
        },
        sink,
        projector,
    )?;
    Ok(DispatchResult {
        full,
        is_error,
        duration_ms,
    })
}

fn record_permission_checks(
    checks: &[CheckRecord],
    sink: &mut JsonlSink,
    projector: &mut TraceProjector,
) -> Result<(), FerricError> {
    for check in checks {
        record_event(permission_event(check), sink, projector)?;
    }
    Ok(())
}

fn record_event(
    event: Event,
    sink: &mut JsonlSink,
    projector: &mut TraceProjector,
) -> Result<(), FerricError> {
    sink.write_event(event.clone())?;
    projector.step(&event);
    Ok(())
}

fn permission_event(check: &CheckRecord) -> Event {
    Event::PermissionCheck {
        path: check.path.display().to_string(),
        decision: check.decision.clone(),
        rule: check.rule.clone(),
        matched: check.matched.clone(),
    }
}

fn prepared_path_state(state: &CandidatePathState) -> PreparedPathStateV1 {
    PreparedPathStateV1 {
        path: state.path.clone(),
        before: prepared_identity(&state.before),
        candidate: prepared_identity(&state.candidate),
    }
}

fn prepared_identity(state: &PathState) -> PreparedPathIdentityV1 {
    match state {
        PathState::Absent => PreparedPathIdentityV1::Absent,
        PathState::File { sha256, bytes, .. } => PreparedPathIdentityV1::File {
            sha256: sha256.clone(),
            bytes: *bytes,
        },
        PathState::Directory => PreparedPathIdentityV1::Directory,
        PathState::Other => PreparedPathIdentityV1::Other,
    }
}

fn syntax_state(state: SyntaxState) -> SyntaxStateV1 {
    match state {
        SyntaxState::Absent => SyntaxStateV1::Absent,
        SyntaxState::Valid => SyntaxStateV1::Valid,
        SyntaxState::Invalid => SyntaxStateV1::Invalid,
        SyntaxState::Unchecked(_) => SyntaxStateV1::Unchecked,
    }
}

fn unsupported_kind(kind: UnsupportedMutationKind) -> UnsupportedMutationKindV1 {
    match kind {
        UnsupportedMutationKind::OpaqueMutation => UnsupportedMutationKindV1::OpaqueMutation,
        UnsupportedMutationKind::UnsupportedOperation => {
            UnsupportedMutationKindV1::UnsupportedOperation
        }
    }
}

fn controlled_target_paths(
    workspace: &Workspace,
    call: &ToolCall,
) -> Result<Vec<String>, FerricError> {
    let keys: &[&str] = match call.name.as_str() {
        "write_file" | "edit_file" | "multi_edit" | "apply_patch" => &["path"],
        _ => &[],
    };
    keys.iter()
        .map(|key| {
            let value = call
                .args
                .get(*key)
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    contract_error(
                        &call.name,
                        format!("missing string target argument {key:?}"),
                    )
                })?;
            let resolved = workspace.resolve(value).map_err(|error| {
                contract_error(&call.name, format!("cannot normalize target: {error}"))
            })?;
            let relative = resolved.strip_prefix(workspace.root()).map_err(|_| {
                contract_error(&call.name, "normalized target escaped the workspace")
            })?;
            Ok(relative.to_string_lossy().replace('\\', "/"))
        })
        .collect()
}

fn edit_preview(call: &ToolCall, tainted: bool) -> EditPreview {
    let mut detail = String::new();
    if tainted {
        detail.push_str(
            "WARNING: this run has ingested untrusted research content, so every mutation is gated.\n",
        );
    }
    detail.push_str(
        &serde_json::to_string_pretty(&call.args).unwrap_or_else(|_| call.args.to_string()),
    );
    EditPreview {
        tool: call.name.clone(),
        targets: ["path", "from", "to", "src", "dest"]
            .iter()
            .filter_map(|key| call.args.get(key).and_then(serde_json::Value::as_str))
            .map(str::to_string)
            .collect(),
        detail,
    }
}

fn diagnostic_sha256(diagnostic: &str, workspace: &Workspace) -> String {
    sha256_bytes(normalize_diagnostic(diagnostic, workspace).as_bytes())
}

fn normalize_diagnostic(diagnostic: &str, workspace: &Workspace) -> String {
    let mut normalized = diagnostic.replace("\r\n", "\n").replace('\r', "\n");
    normalized = normalized
        .split('\n')
        .map(|line| line.trim_end_matches([' ', '\t']))
        .collect::<Vec<_>>()
        .join("\n");

    let mut roots = Vec::new();
    for root in [
        Some(workspace.root().to_path_buf()),
        std::fs::canonicalize(workspace.root()).ok(),
    ]
    .into_iter()
    .flatten()
    {
        let displayed = root.display().to_string();
        roots.push(displayed.clone());
        roots.push(displayed.replace('\\', "/"));
        roots.push(displayed.replace('/', "\\"));
    }
    roots.sort_by_key(|root| std::cmp::Reverse(root.len()));
    roots.dedup();
    for root in roots {
        if !root.is_empty() {
            normalized = normalized.replace(&root, "<workspace>");
        }
    }
    normalized
}

fn contract_error(tool: &str, message: impl Into<String>) -> FerricError {
    FerricError::Other(format!(
        "evidence controlled-dispatch contract violation for {tool}: {}",
        message.into()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_core::HarnessPolicy;
    use ferric_tools::ControlFailure;
    use ferric_trace::{ParsedEvent, TraceReader};

    #[test]
    fn diagnostic_normalization_is_platform_and_workspace_stable() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let display = workspace.root().display().to_string();
        let slash = display.replace('\\', "/");
        let diagnostic = format!("first  \r\n{display}\\src\\lib.rs\t\r{slash}/tests/test.rs   \n");
        assert_eq!(
            normalize_diagnostic(&diagnostic, &workspace),
            "first\n<workspace>\\src\\lib.rs\n<workspace>/tests/test.rs\n"
        );
    }

    #[test]
    fn navigation_exhaustion_uses_has_more_not_a_filled_cap() {
        let observation = NavigationObservation {
            kind: NavigationKind::FindFiles,
            root: ".".to_string(),
            literal: "rs".to_string(),
            result_sha256: "a".repeat(64),
            matches: 2,
            limit: 2,
            cap_reached: true,
            has_more: false,
            model_truncated: false,
        };
        let ObservationDetailV1::Find(mapped) = navigation_observation(&observation).detail else {
            panic!("expected find observation");
        };
        assert!(mapped.exhausted);
    }

    #[test]
    fn verification_rejects_mutation_and_observation_metadata() {
        let observation = ToolObservation::Navigation(NavigationObservation {
            kind: NavigationKind::FindFiles,
            root: ".".to_string(),
            literal: "rs".to_string(),
            result_sha256: "a".repeat(64),
            matches: 0,
            limit: 10,
            cap_reached: false,
            has_more: false,
            model_truncated: false,
        });
        for (effects, observation) in [
            (WorkspaceEffectReport::measured_none(), None),
            (WorkspaceEffectReport::UnmeasuredLegacy, Some(observation)),
        ] {
            let error =
                require_verification_contract("run_check", &effects, &observation, None, false)
                    .unwrap_err();
            assert!(error.to_string().contains("contract violation"));
        }
    }

    #[test]
    fn partial_error_effect_advances_epoch_before_tool_result() {
        let directory = tempfile::tempdir().unwrap();
        let trace = directory.path().join("partial.jsonl");
        let mut sink = JsonlSink::open(&trace, "partial").unwrap();
        let mut projector = TraceProjector::new();
        let mut controller = ControllerState::new(HarnessPolicy::Evidence, Vec::new()).unwrap();
        let call = ToolCall {
            id: "partial".to_string(),
            name: "write_file".to_string(),
            args: serde_json::json!({"path": "partial.txt"}),
        };
        let report = WorkspaceEffectReport::Measured(vec![WorkspaceEffect {
            path: "partial.txt".to_string(),
            kind: WorkspaceEffectKind::Created,
            before: PathState::Absent,
            after: PathState::File {
                sha256: "a".repeat(64),
                bytes: 7,
                lines: 1,
            },
        }]);
        let failure = ControlFailure {
            kind: ControlFailureKind::Io,
            message: "write changed bytes before a later I/O error".to_string(),
            witness: None,
        };

        apply_mutation_outcome(
            1,
            &call,
            &report,
            Some(&failure),
            &mut controller,
            &mut sink,
            &mut projector,
        )
        .unwrap();
        record_result(&call, failure.message, true, 4, &mut sink, &mut projector).unwrap();
        drop(sink);

        assert_eq!(controller.mutation_epoch(), 1);
        let events = TraceReader::open(trace)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(matches!(
            &events[0].event,
            ParsedEvent::Known(Event::WorkspaceEffectRecorded { .. })
        ));
        assert!(matches!(
            &events[1].event,
            ParsedEvent::Known(Event::WorkspaceMutation {
                mutation_epoch: 1,
                ..
            })
        ));
        assert!(matches!(
            &events[2].event,
            ParsedEvent::Known(Event::ToolResult { is_error: true, .. })
        ));
    }

    #[test]
    fn unverifiable_mutation_effects_fail_before_tool_result() {
        let directory = tempfile::tempdir().unwrap();
        for (label, report) in [
            ("unmeasured", WorkspaceEffectReport::UnmeasuredLegacy),
            (
                "opaque",
                WorkspaceEffectReport::Measured(vec![WorkspaceEffect {
                    path: "opaque.txt".to_string(),
                    kind: WorkspaceEffectKind::Opaque,
                    before: PathState::Absent,
                    after: PathState::Other,
                }]),
            ),
        ] {
            let trace = directory.path().join(format!("{label}.jsonl"));
            let mut sink = JsonlSink::open(&trace, label).unwrap();
            let mut projector = TraceProjector::new();
            let mut controller = ControllerState::new(HarnessPolicy::Evidence, Vec::new()).unwrap();
            let call = ToolCall {
                id: label.to_string(),
                name: "write_file".to_string(),
                args: serde_json::json!({"path": "opaque.txt"}),
            };

            let error = apply_mutation_outcome(
                1,
                &call,
                &report,
                None,
                &mut controller,
                &mut sink,
                &mut projector,
            )
            .unwrap_err();
            drop(sink);

            assert!(error.to_string().contains("contract violation"));
            assert_eq!(controller.mutation_epoch(), 0);
            assert!(
                TraceReader::open(trace)
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
                    .iter()
                    .all(|record| !matches!(
                        &record.event,
                        ParsedEvent::Known(Event::ToolResult { .. })
                    ))
            );
        }
    }
}

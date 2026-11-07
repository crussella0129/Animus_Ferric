//! Trace and evidence builders shared by the in-module tests of `replay`,
//! `trace_structure` and `controller`.

use ferric_core::{ActionProtocol, HarnessPolicy, ToolCall};
use ferric_trace::{
    CONTROLLER_RECORD_VERSION, Event, FileObservationV1, LineRangeV1, ObservationDetailV1,
    ObservationV1, PathEffectKind, PathEffectV1, WorkspaceEffectV1,
};

pub(crate) fn sha(byte: char) -> String {
    byte.to_string().repeat(64)
}

pub(crate) fn tool_call(id: &str, name: &str, args: serde_json::Value) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        args,
    }
}

pub(crate) fn session_start() -> Event {
    Event::SessionStart {
        workspace: "/ws".to_string(),
        resumed_from: None,
    }
}

pub(crate) fn policy_event(
    protocol: ActionProtocol,
    harness_policy: HarnessPolicy,
    truncation_limit: usize,
) -> Event {
    Event::PolicySelected {
        tier: ferric_core::Tier::Nano,
        protocol,
        harness_policy,
        max_turns: 15,
        max_tools: 10,
        prompt_budget_tokens: 2_800,
        max_output_tokens: 512,
        truncation_limit,
        tier_source: ferric_core::TierSource::Params.label().to_string(),
    }
}

pub(crate) fn call_event(call: &ToolCall) -> Event {
    Event::ToolCall {
        id: call.id.clone(),
        name: call.name.clone(),
        args: call.args.clone(),
    }
}

pub(crate) fn result_event(id: &str, name: &str, output: &str, is_error: bool) -> Event {
    Event::ToolResult {
        id: id.to_string(),
        name: name.to_string(),
        output: output.to_string(),
        is_error,
        duration_ms: 1,
    }
}

/// A complete read of a `lines`-line file of `2 * lines` bytes.
pub(crate) fn full_file(path: &str, digest: &str, lines: u64) -> ObservationV1 {
    ObservationV1 {
        version: CONTROLLER_RECORD_VERSION,
        detail: ObservationDetailV1::File(FileObservationV1 {
            path: path.to_string(),
            sha256: digest.to_string(),
            total_bytes: lines.saturating_mul(2),
            total_lines: lines,
            requested_range: None,
            returned_range: (lines > 0).then_some(LineRangeV1 {
                start: 1,
                end: lines,
            }),
            complete: true,
            model_truncated: false,
        }),
    }
}

/// One file modified into a 6-byte, 3-line postimage at `epoch`.
pub(crate) fn modified(path: &str, before: &str, after: &str, epoch: u64) -> WorkspaceEffectV1 {
    WorkspaceEffectV1 {
        version: CONTROLLER_RECORD_VERSION,
        mutation_epoch: epoch,
        effects: vec![PathEffectV1 {
            path: path.to_string(),
            kind: PathEffectKind::Modified,
            before_sha256: Some(before.to_string()),
            after_sha256: Some(after.to_string()),
            after_bytes: Some(6),
            after_lines: Some(3),
        }],
    }
}

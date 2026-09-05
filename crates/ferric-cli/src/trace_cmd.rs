//! `ferric trace cat` — the DERIVED human view of a JSONL trace. The JSONL
//! file is canonical (ADR-002); rendering must never fail on unknown events.

use std::path::Path;
use std::process::ExitCode;

use ferric_trace::{Event, ObservationDetailV1, ParsedEvent, TraceReader};

pub fn trace_cat(path: &Path) -> ExitCode {
    let reader = match TraceReader::open(path) {
        Ok(reader) => reader,
        Err(e) => {
            eprintln!("cannot open {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    for record in reader {
        match record {
            Ok(record) => println!("{}", render(&record.session, record.seq, &record.event)),
            Err(e) => {
                eprintln!("bad trace line: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}

fn render(session: &str, seq: u64, event: &ParsedEvent) -> String {
    let body = match event {
        ParsedEvent::Known(Event::SessionStart {
            workspace,
            resumed_from,
        }) => match resumed_from {
            Some(prior) => format!("session start (workspace: {workspace}, resumed from {prior})"),
            None => format!("session start (workspace: {workspace})"),
        },
        ParsedEvent::Known(Event::SessionEnd { reason }) => {
            format!("session end ({reason})")
        }
        ParsedEvent::Known(Event::SessionPaused { reason }) => {
            format!("session paused ({reason})")
        }
        ParsedEvent::Known(Event::ToolCall { id, name, args }) => {
            format!("tool call {name} [{id}] args={args}")
        }
        ParsedEvent::Known(Event::ToolResult {
            id,
            name,
            output,
            is_error,
            duration_ms,
        }) => {
            let status = if *is_error { "ERROR" } else { "ok" };
            let preview: String = output.chars().take(120).collect();
            let ellipsis = if output.chars().count() > 120 {
                "…"
            } else {
                ""
            };
            format!("tool result {name} [{id}] {status} {duration_ms}ms: {preview}{ellipsis}")
        }
        ParsedEvent::Known(Event::ObservationRecorded {
            turn,
            call_id,
            observation,
        }) => {
            let detail = match &observation.detail {
                ObservationDetailV1::File(file) => format!(
                    "file {} sha256 {} ({} bytes, complete {}, truncated {})",
                    file.path, file.sha256, file.total_bytes, file.complete, file.model_truncated
                ),
                ObservationDetailV1::Search(search) => format!(
                    "literal search {:?} under {} ({} matches, exhausted {})",
                    search.literal, search.root, search.match_count, search.exhausted
                ),
                ObservationDetailV1::Find(find) => format!(
                    "literal find {:?} under {} ({} matches, exhausted {})",
                    find.literal, find.root, find.match_count, find.exhausted
                ),
            };
            format!(
                "turn {turn} observation v{} [{call_id}]: {detail}",
                observation.version
            )
        }
        ParsedEvent::Known(Event::ControllerBlocked {
            turn,
            call_id,
            tool,
            block,
        }) => format!(
            "turn {turn} controller blocked {tool} [{call_id}] at epoch {}: {:?} (paths [{}], check {})",
            block.mutation_epoch,
            block.reason,
            block.paths.join(", "),
            block.check_name.as_deref().unwrap_or("none")
        ),
        ParsedEvent::Known(Event::WorkspaceEffectRecorded {
            turn,
            call_id,
            tool,
            effect,
        }) => format!(
            "turn {turn} measured effect by {tool} [{call_id}] advanced epoch to {}: [{}]",
            effect.mutation_epoch,
            effect
                .effects
                .iter()
                .map(|item| format!("{}:{:?}", item.path, item.kind))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ParsedEvent::Known(Event::VerificationCheckRecorded {
            turn,
            call_id,
            check,
        }) => format!(
            "turn {turn} verification check {} [{call_id}] attempt {} at epoch {}: {:?}{}",
            check.name,
            check.attempt,
            check.mutation_epoch,
            check.outcome,
            check
                .diagnostic_sha256
                .as_deref()
                .map_or_else(String::new, |digest| format!(" ({digest})"))
        ),
        ParsedEvent::Known(Event::WorkspaceMutation {
            turn,
            tool,
            mutation_epoch,
        }) => format!(
            "turn {turn} workspace mutation by {tool} advanced evidence epoch to {mutation_epoch}"
        ),
        ParsedEvent::Known(Event::VerificationCheckPassed {
            turn,
            name,
            mutation_epoch,
        }) => format!(
            "turn {turn} verification check {name} passed at mutation epoch {mutation_epoch}"
        ),
        ParsedEvent::Known(Event::CompletionGate {
            mutation_epoch,
            required_checks,
            fresh_checks,
            decision,
        }) => format!(
            "completion gate {decision} at epoch {mutation_epoch}: {}/{} fresh checks",
            fresh_checks.len(),
            required_checks.len()
        ),
        ParsedEvent::Known(Event::MainActionBudget { turn, budget }) => format!(
            "main action {turn}: output cap {}, requested {:?}, declared context {:?}, source {:?}",
            budget.effective, budget.requested, budget.declared_ctx, budget.source
        ),
        ParsedEvent::Known(Event::PolicySelected {
            tier,
            protocol,
            harness_policy,
            max_turns,
            max_tools,
            prompt_budget_tokens,
            max_output_tokens,
            truncation_limit,
            tier_source,
        }) => format!(
            "policy selected: {tier:?} (from {tier_source})/{protocol:?}, harness {harness_policy} (turns {max_turns}, tools {max_tools}, prompt budget {prompt_budget_tokens}, output budget {max_output_tokens}, tool output cap {truncation_limit})"
        ),
        ParsedEvent::Known(Event::PromptComposed {
            output_id,
            output_version,
            composed_of,
        }) => format!(
            "prompt composed: {output_id} v{output_version} from [{}]",
            composed_of
                .iter()
                .map(|(id, v)| format!("{id}@{v}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ParsedEvent::Known(Event::SessionPrompt { system, user, .. }) => {
            format!(
                "session prompt: system {} chars, user {} chars",
                system.chars().count(),
                user.chars().count()
            )
        }
        ParsedEvent::Known(Event::RecoveryCheckpoint { state }) => format!(
            "recovery checkpoint v{} (next turn {}, {} messages{})",
            state.version,
            state.next_turn,
            state.messages.len(),
            if state.pending_input.is_some() {
                ", awaiting user input"
            } else {
                ""
            }
        ),
        ParsedEvent::Known(Event::ControllerCheckpoint { state }) => format!(
            "controller checkpoint v{} (harness {}, epoch {}, {} file observations, {} check executions)",
            state.version,
            state.harness_policy,
            state.mutation_epoch,
            state.file_evidence.len(),
            state.check_executions.len()
        ),
        ParsedEvent::Known(Event::RecoveryPacketInjected { packet, message }) => format!(
            "recovery packet v{} (reason {}, epoch {}, {} reread paths, {} chars)",
            packet.version,
            packet.pause_reason,
            packet.mutation_epoch,
            packet.reread_paths.len(),
            message.chars().count()
        ),
        ParsedEvent::Known(Event::ResumePrompt { user, .. }) => {
            format!("resume prompt: {} chars", user.chars().count())
        }
        ParsedEvent::Known(Event::TurnStart { turn }) => format!("turn {turn} start"),
        ParsedEvent::Known(Event::TurnEnd {
            turn,
            text,
            tool_call_count,
            input_tokens,
            output_tokens,
            truncated,
        }) => {
            let preview = match text {
                Some(t) => {
                    let p: String = t.chars().take(80).collect();
                    let ellipsis = if t.chars().count() > 80 { "…" } else { "" };
                    format!(" text: {p}{ellipsis}")
                }
                None => String::new(),
            };
            let truncated_note = if *truncated { " TRUNCATED" } else { "" };
            format!(
                "turn {turn} end ({tool_call_count} tool calls, tokens in/out {}/{}){truncated_note}{preview}",
                input_tokens.map_or("?".to_string(), |t| t.to_string()),
                output_tokens.map_or("?".to_string(), |t| t.to_string()),
            )
        }
        ParsedEvent::Known(Event::ActionsProposed { turn, calls }) => format!(
            "turn {turn} proposed {} action(s): [{}]",
            calls.len(),
            calls
                .iter()
                .map(|call| call.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ParsedEvent::Known(Event::TurnCommitted {
            turn,
            dispatched,
            errored,
            stop_reason,
            snapshot_commit,
        }) => format!(
            "turn {turn} committed ({dispatched} result(s), {errored} error(s), stop {}, snapshot {})",
            stop_reason.as_deref().unwrap_or("none"),
            snapshot_commit.as_deref().unwrap_or("unavailable")
        ),
        ParsedEvent::Known(Event::PromptAssembled {
            turn,
            message_count,
            chars,
            offered_tools,
        }) => format!(
            "turn {turn} prompt assembled: {message_count} messages, {chars} chars, tools [{}]",
            offered_tools.join(", ")
        ),
        ParsedEvent::Known(Event::ConstraintApplied { kind }) => {
            format!("constraint applied: {kind}")
        }
        ParsedEvent::Known(Event::RepetitionGuard { action }) => {
            format!("repetition guard: {action}")
        }
        ParsedEvent::Known(Event::NoProgressGuard { action }) => {
            format!("no-progress guard: {action}")
        }
        ParsedEvent::Known(Event::FailureGuard { action }) => {
            format!("repeated-failure guard: {action}")
        }
        ParsedEvent::Known(Event::OscillationGuard { action }) => {
            format!("oscillation guard: {action}")
        }
        ParsedEvent::Known(Event::PermissionCheck {
            path,
            decision,
            rule,
            matched,
        }) => {
            let detail = match (rule, matched) {
                (Some(rule), Some(matched)) => format!(" ({rule}: {matched})"),
                (Some(rule), None) => format!(" ({rule})"),
                _ => String::new(),
            };
            format!("permission {decision}: {path}{detail}")
        }
        ParsedEvent::Known(Event::Note { text }) => format!("note: {text}"),
        ParsedEvent::Known(Event::HistoryCompacted {
            through_turn,
            dropped_turns,
            summary,
        }) => {
            let preview: String = summary.chars().take(120).collect();
            let ellipsis = if summary.chars().count() > 120 {
                "…"
            } else {
                ""
            };
            format!(
                "history compacted: folded {dropped_turns} turns (through turn {through_turn}): {preview}{ellipsis}"
            )
        }
        ParsedEvent::Unknown(raw) => {
            let kind = raw
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("<untyped>");
            format!("[unknown event: {kind}]")
        }
    };
    format!("{session}#{seq:<4} {body}")
}

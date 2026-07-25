//! `ferric trace cat` — the DERIVED human view of a JSONL trace. The JSONL
//! file is canonical (ADR-002); rendering must never fail on unknown events.

use std::path::Path;
use std::process::ExitCode;

use ferric_trace::{Event, ParsedEvent, TraceReader};

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
        ParsedEvent::Known(Event::PolicySelected {
            tier,
            protocol,
            max_turns,
            max_tools,
            prompt_budget_tokens,
            max_output_tokens,
        }) => format!(
            "policy selected: {tier:?}/{protocol:?} (turns {max_turns}, tools {max_tools}, prompt budget {prompt_budget_tokens}, output budget {max_output_tokens})"
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

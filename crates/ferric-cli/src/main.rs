//! The `ferric` binary. s0 surface: `--version` and `trace cat <file.jsonl>`.
//!
//! The trace rendering here is a DERIVED view; the JSONL file is canonical
//! (ADR-002). Arg parsing is std-only — clap arrives when the CLI grows.

use std::process::ExitCode;

use ferric_trace::{Event, ParsedEvent, TraceReader};

const USAGE: &str = "usage:
  ferric --version
  ferric trace cat <file.jsonl>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["--version" | "-V"] => {
            println!("ferric {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        ["trace", "cat", path] => trace_cat(path),
        [] => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
        _ => {
            eprintln!("unrecognized arguments: {}\n{USAGE}", args.join(" "));
            ExitCode::FAILURE
        }
    }
}

fn trace_cat(path: &str) -> ExitCode {
    let reader = match TraceReader::open(path) {
        Ok(reader) => reader,
        Err(e) => {
            eprintln!("cannot open {path}: {e}");
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
        ParsedEvent::Known(Event::SessionStart { workspace }) => {
            format!("session start (workspace: {workspace})")
        }
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
        ParsedEvent::Known(Event::Note { text }) => format!("note: {text}"),
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

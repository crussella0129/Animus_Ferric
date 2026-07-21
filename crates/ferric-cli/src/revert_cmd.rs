use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use ferric_trace::TraceReader;

#[derive(Args)]
pub struct RevertArgs {
    /// The path to the trace file to revert
    pub trace: PathBuf,
    /// The turn number to revert the workspace to
    pub turn: u32,
}

pub fn run_revert(args: RevertArgs) -> ExitCode {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    rt.block_on(async {
        match revert_inner(args).await {
            Ok(_) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("revert failed: {}", e);
                ExitCode::FAILURE
            }
        }
    })
}

#[allow(clippy::collapsible_if)]
async fn revert_inner(args: RevertArgs) -> Result<(), String> {
    let reader = TraceReader::open(&args.trace).map_err(|e| format!("cannot open trace: {e}"))?;
    let mut session_id = String::new();
    let mut target_turn_found = false;

    for record_res in reader {
        let record = record_res.map_err(|e| format!("bad trace line: {e}"))?;
        if session_id.is_empty() {
            session_id = record.session.clone();
        }

        // Let's see if we need to do this or if we just want to execute the VCS revert.
        if let ferric_trace::ParsedEvent::Known(ferric_trace::Event::TurnEnd { turn, .. }) =
            record.event
        {
            if turn == args.turn {
                target_turn_found = true;
            }
        }
    }

    if !target_turn_found {
        return Err(format!("turn {} not found in trace", args.turn));
    }

    // Call VCS revert
    // Assuming the workspace root is the current directory for now, or we can find it via the trace
    let workspace_root = std::env::current_dir().map_err(|e| format!("io error: {e}"))?;
    let vcs = ferric_vcs::Vcs::new(workspace_root);

    println!(
        "Reverting workspace to session {}, turn {}...",
        session_id, args.turn
    );
    vcs.revert(&session_id, args.turn)
        .await
        .map_err(|e| format!("vcs error: {e}"))?;

    println!("Workspace reverted successfully.");

    // Note: Trace truncation is complex if TraceReader doesn't track offsets.
    // The simplest way to truncate is to read all valid lines up to and including the target TurnEnd,
    // and rewrite the file.
    truncate_trace(&args.trace, args.turn)?;

    Ok(())
}

#[allow(clippy::collapsible_if)]
fn truncate_trace(path: &std::path::Path, target_turn: u32) -> Result<(), String> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("read error: {e}"))?;
    let mut kept_lines = Vec::new();
    let mut hit_target = false;

    for line in content.lines() {
        kept_lines.push(line);
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(event) = parsed.get("event") {
                if event.get("type").and_then(|t| t.as_str()) == Some("turn_end") {
                    if let Some(turn) = event.get("turn").and_then(|t| t.as_u64()) {
                        if turn as u32 == target_turn {
                            hit_target = true;
                            break;
                        }
                    }
                }
            }
        }
    }

    if hit_target {
        let new_content = kept_lines.join("\n") + "\n";
        std::fs::write(path, new_content).map_err(|e| format!("write error: {e}"))?;
        println!("Trace truncated to turn {}.", target_turn);
    }

    Ok(())
}

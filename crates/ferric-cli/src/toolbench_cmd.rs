//! `ferric toolbench` — per-tool fire-rate benchmark.
//!
//! Measures, for each registered tool, how often the model emits a call to
//! exactly that tool. Critically (ADR-022) it parses the completion with the
//! SAME parser the agent loop uses for the active protocol — native
//! `tool_calls`, constrained `{tool,args}` JSON, or scraped `<tool_call>` XML —
//! so the fire rate reflects the real path, not a native-only check that always
//! read empty (the s6 0.0% bug).

use clap::Args;
use std::path::PathBuf;
use std::process::ExitCode;

use crate::backend::BackendOpts;
use crate::query::ProtocolArg;

// Used by `extract_action` (and its tests); excluded from the backend-free,
// non-test bin build so it stays unused-import-clean under `-D warnings`.
#[cfg(any(feature = "backend-openai", test))]
use ferric_core::{ActionProtocol, ToolCall};
#[cfg(any(feature = "backend-openai", test))]
use ferric_provider::Completion;

#[cfg(feature = "backend-openai")]
use {
    crate::backend::create_provider,
    ferric_core::{Message, ModelProfile, policy_for},
    ferric_loop::{action_schema, select_protocol},
    ferric_provider::{CompletionRequest, Constraint, SamplingParams, ToolDescriptor},
    ferric_tools::Registry,
};

#[derive(Args)]
pub struct ToolbenchArgs {
    #[command(flatten)]
    pub backend_opts: BackendOpts,

    /// Action protocol to test (default: chosen from the backend's capabilities)
    #[arg(long, value_enum)]
    pub protocol: Option<ProtocolArg>,

    /// Number of iterations per tool to test fire rate
    #[arg(long, default_value_t = 10)]
    pub iterations: u32,

    /// Comma-separated model list for a fleet sweep (overrides `--model` /
    /// `--model-file` per run) — benches each and prints a sorted leaderboard.
    #[arg(long)]
    pub models: Option<String>,

    /// Write a Markdown report here (+ a sibling `.jsonl`). Without it, the
    /// report only prints to stdout.
    #[arg(long)]
    pub report: Option<PathBuf>,

    /// Cap the active tool ring (ADR-028) — bench exactly rings `0..=N`.
    #[arg(long)]
    pub max_ring: Option<u8>,

    /// Sweep rings 0, 1, … and report the highest ring this model reliably
    /// drives — the recommended `--max-ring`. Supersedes `--max-ring`.
    #[arg(long)]
    pub calibrate_rings: bool,

    /// With `--calibrate-rings`, persist each model's recommended ring into
    /// `<dir>/model_profiles.json` (`calibrated_ring`) so `ferric query` applies
    /// it automatically (ADR-029). Default matches `ferric bench`'s results dir.
    #[arg(long, default_value = "benchmarks")]
    pub profile_dir: std::path::PathBuf,

    /// Parameter count (billions) the bench profile is built at — sets the tier,
    /// hence the ring ceiling. Default 8.0 (Small → rings 0–1); `--params-b 20`
    /// reaches Medium (rings 0–2) so `--calibrate-rings` can sweep Ring 2.
    #[arg(long, default_value_t = 8.0)]
    pub params_b: f32,
}

/// The classified outcome of one toolbench iteration. This is what turns the
/// bench from a pass/fail counter into a diagnostic: it says *why* a model
/// missed, so a user can judge whether a smaller model is still good enough.
#[cfg(any(feature = "backend-openai", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Called the target tool with every schema-required arg present.
    Success,
    /// Called a real, but different, tool.
    WrongTool(String),
    /// Called the target tool but a schema-required arg is missing.
    MalformedArgs,
    /// Produced no parseable action (native: no tool_calls; text: empty).
    NoAction,
    /// Produced non-empty action-shaped text that did not parse.
    ParseError,
}

#[cfg(any(feature = "backend-openai", test))]
impl Outcome {
    pub fn is_success(&self) -> bool {
        matches!(self, Outcome::Success)
    }

    /// Stable label for the failure histogram / JSONL rows.
    pub fn label(&self) -> &'static str {
        match self {
            Outcome::Success => "success",
            Outcome::WrongTool(_) => "wrong_tool",
            Outcome::MalformedArgs => "malformed_args",
            Outcome::NoAction => "no_action",
            Outcome::ParseError => "parse_error",
        }
    }
}

/// Classify one completion against the `target` tool (and its `schema`) using
/// the SAME parser the agent loop uses for `protocol`. Distinguishes "nothing"
/// (NoAction) from "action-shaped but unparseable" (ParseError), "called a
/// different tool" (WrongTool), and "right tool, missing a required arg"
/// (MalformedArgs). The arg check is a lightweight required-keys check against
/// `schema.required`, not full JSON-Schema validation.
#[cfg(any(feature = "backend-openai", test))]
pub fn classify(
    protocol: ActionProtocol,
    completion: &Completion,
    target: &str,
    schema: &serde_json::Value,
) -> Outcome {
    let text = completion.message.text.as_deref().unwrap_or_default();
    let parsed: Result<Option<ToolCall>, ()> = match protocol {
        ActionProtocol::NativeTools => Ok(completion.message.tool_calls.first().cloned()),
        ActionProtocol::ConstrainedJson | ActionProtocol::Plan => {
            if text.trim().is_empty() {
                Ok(None)
            } else {
                ferric_loop::parse_json_action(0, text)
                    .map(Some)
                    .map_err(|_| ())
            }
        }
        ActionProtocol::TextXml => {
            if text.trim().is_empty() {
                Ok(None)
            } else {
                ferric_loop::parse_action(0, text).map(Some).map_err(|_| ())
            }
        }
    };
    let call = match parsed {
        Ok(Some(call)) => call,
        Ok(None) => return Outcome::NoAction,
        Err(()) => return Outcome::ParseError,
    };
    if call.name != target {
        return Outcome::WrongTool(call.name);
    }
    if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
        for key in required.iter().filter_map(|k| k.as_str()) {
            if call.args.get(key).is_none() {
                return Outcome::MalformedArgs;
            }
        }
    }
    Outcome::Success
}

/// One tool's diagnostic stats. `histogram` is `(outcome label, count)` sorted
/// by label (ADR-008 deterministic ordering).
#[cfg(any(feature = "backend-openai", test))]
pub struct ToolStat {
    pub name: String,
    pub fires: u32,
    pub success: u32,
    pub histogram: Vec<(String, u32)>,
}

#[cfg(any(feature = "backend-openai", test))]
impl ToolStat {
    pub fn rate(&self) -> f64 {
        if self.fires == 0 {
            0.0
        } else {
            self.success as f64 / self.fires as f64
        }
    }
}

/// The whole bench's diagnostic summary — the input to the report writers.
#[cfg(any(feature = "backend-openai", test))]
pub struct BenchSummary {
    pub model: String,
    pub backend: String,
    pub protocol: String,
    pub iterations: u32,
    pub per_tool: Vec<ToolStat>,
}

#[cfg(any(feature = "backend-openai", test))]
impl BenchSummary {
    pub fn overall(&self) -> (u32, u32) {
        let success = self.per_tool.iter().map(|t| t.success).sum();
        let fires = self.per_tool.iter().map(|t| t.fires).sum();
        (success, fires)
    }

    pub fn overall_rate(&self) -> f64 {
        let (success, fires) = self.overall();
        if fires == 0 {
            0.0
        } else {
            success as f64 / fires as f64
        }
    }
}

/// Acceptability band for a success rate in `0.0..=1.0`. This is the readout
/// that answers "is this model good enough" as you dial it down.
#[cfg(any(feature = "backend-openai", test))]
pub fn verdict(rate: f64) -> &'static str {
    if rate >= 0.90 {
        "solid"
    } else if rate >= 0.70 {
        "marginal"
    } else {
        "unreliable"
    }
}

/// The recommended `--max-ring` from a ring-by-ring calibration: the highest
/// ring with an unbroken `solid` prefix from ring 0. `None` means even ring 0
/// is not solid — the model can't reliably drive the core (ADR-028/019).
#[cfg(any(feature = "backend-openai", test))]
pub fn recommend_max_ring(ring_solid: &[bool]) -> Option<u8> {
    if ring_solid.first() != Some(&true) {
        return None;
    }
    let mut r = 0u8;
    for &solid in &ring_solid[1..] {
        if !solid {
            break;
        }
        r += 1;
    }
    Some(r)
}

/// Render the human-facing Markdown report.
#[cfg(any(feature = "backend-openai", test))]
pub fn render_report(s: &BenchSummary) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "# Toolbench Report\n");
    let _ = writeln!(out, "- **Model:** {}", s.model);
    let _ = writeln!(out, "- **Backend:** {}", s.backend);
    let _ = writeln!(out, "- **Protocol:** {}", s.protocol);
    let _ = writeln!(out, "- **Iterations per tool:** {}\n", s.iterations);
    let _ = writeln!(out, "| Tool | Success | Rate | Verdict | Failures |");
    let _ = writeln!(out, "|------|---------|------|---------|----------|");
    for t in &s.per_tool {
        let rate = t.rate();
        let failures: Vec<String> = t
            .histogram
            .iter()
            .filter(|(label, _)| label != "success")
            .map(|(label, n)| format!("{label}×{n}"))
            .collect();
        let failures = if failures.is_empty() {
            "—".to_string()
        } else {
            failures.join(", ")
        };
        let _ = writeln!(
            out,
            "| {} | {}/{} | {:.1}% | {} | {} |",
            t.name,
            t.success,
            t.fires,
            rate * 100.0,
            verdict(rate),
            failures,
        );
    }
    let (os, of) = s.overall();
    let orate = s.overall_rate();
    let _ = writeln!(
        out,
        "\n**Overall:** {os}/{of} ({:.1}%) — **{}**",
        orate * 100.0,
        verdict(orate),
    );
    out
}

/// Machine-readable rows: one per tool, plus a trailing `__overall__` row.
#[cfg(any(feature = "backend-openai", test))]
pub fn summary_rows(s: &BenchSummary) -> Vec<serde_json::Value> {
    let mut rows: Vec<serde_json::Value> = s
        .per_tool
        .iter()
        .map(|t| {
            let hist: serde_json::Map<String, serde_json::Value> = t
                .histogram
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::json!(v)))
                .collect();
            serde_json::json!({
                "model": s.model,
                "tool": t.name,
                "fires": t.fires,
                "success": t.success,
                "rate": t.rate(),
                "verdict": verdict(t.rate()),
                "histogram": hist,
            })
        })
        .collect();
    let (os, of) = s.overall();
    rows.push(serde_json::json!({
        "model": s.model,
        "tool": "__overall__",
        "fires": of,
        "success": os,
        "rate": s.overall_rate(),
        "verdict": verdict(s.overall_rate()),
        "backend": s.backend,
        "protocol": s.protocol,
    }));
    rows
}

/// Render a cross-model **leaderboard** (the fleet-calibration headline):
/// `model | protocol | success | rate | verdict`, sorted best→worst. This is
/// the "which model is good enough" readout as you dial down the fleet.
#[cfg(any(feature = "backend-openai", test))]
pub fn render_leaderboard(summaries: &[BenchSummary]) -> String {
    use std::fmt::Write;
    let mut ranked: Vec<&BenchSummary> = summaries.iter().collect();
    ranked.sort_by(|a, b| {
        b.overall_rate()
            .partial_cmp(&a.overall_rate())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut out = String::new();
    let _ = writeln!(out, "# Fleet Leaderboard\n");
    let _ = writeln!(out, "| Model | Protocol | Success | Rate | Verdict |");
    let _ = writeln!(out, "|-------|----------|---------|------|---------|");
    for s in ranked {
        let (success, fires) = s.overall();
        let rate = s.overall_rate();
        let _ = writeln!(
            out,
            "| {} | {} | {}/{} | {:.1}% | {} |",
            s.model,
            s.protocol,
            success,
            fires,
            rate * 100.0,
            verdict(rate),
        );
    }
    out
}

#[cfg(feature = "backend-openai")]
fn build_request(
    protocol: ActionProtocol,
    all_tools: &[ToolDescriptor],
    schema: &serde_json::Value,
    tool_name: &str,
) -> CompletionRequest {
    let system_prompt = match protocol {
        ActionProtocol::NativeTools => {
            "You are a tool-calling assistant. Call exactly the tool the user names, with \
             valid arguments. Output only the tool call."
        }
        ActionProtocol::ConstrainedJson => {
            "Respond with exactly one JSON object and nothing else: \
             {\"tool\": \"<name>\", \"args\": { ... }}."
        }
        ActionProtocol::TextXml => {
            "Respond with exactly one XML tool call: \
             <tool_call><name>TOOL</name><args>{\"arg\": \"value\"}</args></tool_call>."
        }
        ActionProtocol::Plan => "Respond with exactly one Plan object: {\"plan\": \"...\"}.",
    };
    let user_prompt =
        format!("Invoke the `{tool_name}` tool with dummy data that matches its schema.");
    // ADR-010: constraint XOR tools, by construction.
    let (tools, constraint) = match protocol {
        ActionProtocol::NativeTools => (all_tools.to_vec(), None),
        ActionProtocol::ConstrainedJson => {
            (Vec::new(), Some(Constraint::JsonSchema(schema.clone())))
        }
        ActionProtocol::Plan => (Vec::new(), None),
        ActionProtocol::TextXml => (Vec::new(), None),
    };
    CompletionRequest {
        messages: vec![Message::system(system_prompt), Message::user(user_prompt)],
        tools,
        sampling: SamplingParams {
            temperature: 0.0,
            max_tokens: 256,
            ..SamplingParams::default()
        },
        constraint,
    }
}

/// Bench one model: run every tool `iterations` times, classify each outcome,
/// print live progress, and return its `BenchSummary`. Shared by the
/// single-model report and the fleet leaderboard.
#[cfg(feature = "backend-openai")]
async fn bench_model(
    provider: &(dyn ferric_provider::Provider + Send + Sync),
    protocol: ActionProtocol,
    all_tools: &[ToolDescriptor],
    schema: &serde_json::Value,
    iterations: u32,
    model: &str,
) -> Result<BenchSummary, String> {
    use std::collections::BTreeMap;
    let mut per_tool: Vec<ToolStat> = Vec::new();
    for tool in all_tools {
        let mut hist: BTreeMap<&'static str, u32> = BTreeMap::new();
        print!("Testing tool '{:<15}': ", tool.name);
        for _ in 0..iterations {
            let request = build_request(protocol, all_tools, schema, &tool.name);
            let completion = provider
                .complete(request, None)
                .await
                .map_err(|e| format!("provider error: {e}"))?;
            let outcome = classify(protocol, &completion, &tool.name, &tool.input_schema);
            *hist.entry(outcome.label()).or_insert(0) += 1;
            use std::io::Write;
            print!("{}", if outcome.is_success() { "." } else { "F" });
            let _ = std::io::stdout().flush();
        }
        let success = hist.get("success").copied().unwrap_or(0);
        let histogram: Vec<(String, u32)> =
            hist.iter().map(|(k, v)| ((*k).to_string(), *v)).collect();
        let stat = ToolStat {
            name: tool.name.clone(),
            fires: iterations,
            success,
            histogram,
        };
        println!(
            " [{} / {}] ({:.1}%) {}",
            success,
            iterations,
            stat.rate() * 100.0,
            verdict(stat.rate()),
        );
        per_tool.push(stat);
    }
    Ok(BenchSummary {
        model: model.to_string(),
        backend: provider.id().to_string(),
        protocol: format!("{protocol:?}"),
        iterations,
        per_tool,
    })
}

#[cfg(feature = "backend-openai")]
pub fn run_toolbench(args: ToolbenchArgs) -> ExitCode {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("tokio runtime error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let result = runtime.block_on(async {
        let mut registry = Registry::new();
        ferric_tools::register_builtin_tools(&mut registry);

        let profile = ModelProfile {
            params_b: args.params_b,
            quant: "Q4".to_string(),
            ctx: 4096,
            family: "unknown".to_string(),
            measured_level: None,
        };
        let mut policy = policy_for(&profile);
        // `--max-ring` benches exactly rings 0..=N (ADR-028).
        policy.max_ring = args.max_ring;

        let all_tools: Vec<ToolDescriptor> = registry
            .tools_for_policy(&policy)
            .into_iter()
            .map(|spec| ToolDescriptor {
                name: spec.name,
                description: spec.description,
                input_schema: spec.input_schema,
            })
            .collect();
        let schema = action_schema(&all_tools);

        let protocol_override = args.protocol.map(ActionProtocol::from);

        let write_outputs = |path: &std::path::Path,
                             body: &str,
                             rows: Vec<serde_json::Value>|
         -> Result<(), String> {
            std::fs::write(path, body).map_err(|e| format!("write {}: {e}", path.display()))?;
            let jsonl: String = rows
                .iter()
                .map(|r| r.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            let jsonl_path = path.with_extension("jsonl");
            std::fs::write(&jsonl_path, jsonl)
                .map_err(|e| format!("write {}: {e}", jsonl_path.display()))?;
            println!("Wrote {} + {}", path.display(), jsonl_path.display());
            Ok(())
        };

        if args.calibrate_rings {
            // Ring calibration: bench each model ring-by-ring and report the
            // highest ring it reliably drives (the recommended --max-ring).
            let model_list: Vec<String> = match &args.models {
                Some(m) => m
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
                None => vec![
                    args.backend_opts
                        .model
                        .clone()
                        .unwrap_or_else(|| "model".to_string()),
                ],
            };
            let mut rows: Vec<serde_json::Value> = Vec::new();
            for model in &model_list {
                println!("\n=== calibrating {model} ===");
                let mut opts = args.backend_opts.clone();
                if args.models.is_some() {
                    opts.model = Some(model.clone());
                }
                let provider_box = match create_provider(&opts).await {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("skip {model}: {e}");
                        continue;
                    }
                };
                let provider = provider_box.as_ref();
                let mut results: Vec<(u8, usize, f64)> = Vec::new();
                let mut solids: Vec<bool> = Vec::new();
                let mut prev_len = usize::MAX;
                let mut proto_label = String::new();
                for ring_cap in 0u8..=8 {
                    let mut p = policy.clone();
                    p.max_ring = Some(ring_cap);
                    let ring_tools: Vec<ToolDescriptor> = registry
                        .tools_for_policy(&p)
                        .into_iter()
                        .map(|spec| ToolDescriptor {
                            name: spec.name,
                            description: spec.description,
                            input_schema: spec.input_schema,
                        })
                        .collect();
                    if ring_tools.len() == prev_len {
                        break; // this ring added no tools → max ring reached
                    }
                    let ring_schema = action_schema(&ring_tools);
                    let proto = select_protocol(&p, &provider.capabilities(), protocol_override);
                    proto_label = format!("{proto:?}");
                    let summary = bench_model(
                        provider,
                        proto,
                        &ring_tools,
                        &ring_schema,
                        args.iterations,
                        model,
                    )
                    .await?;
                    let rate = summary.overall_rate();
                    results.push((ring_cap, ring_tools.len(), rate));
                    solids.push(verdict(rate) == "solid");
                    rows.push(serde_json::json!({
                        "model": model, "ring": ring_cap, "tools": ring_tools.len(),
                        "rate": rate, "verdict": verdict(rate),
                    }));
                    prev_len = ring_tools.len();
                }
                println!("\n  ring | tools |   rate | verdict");
                println!("  -----|-------|--------|----------");
                for (r, t, rate) in &results {
                    println!(
                        "  {r:>4} | {t:>5} | {:>5.1}% | {}",
                        rate * 100.0,
                        verdict(*rate)
                    );
                }
                match recommend_max_ring(&solids) {
                    Some(r) => {
                        println!("  → Recommended --max-ring {r} (solid through ring {r})");
                        // Persist the earned ring so `ferric query` auto-applies it (ADR-029).
                        match ferric_bench::write_calibrated_ring(
                            &args.profile_dir,
                            model,
                            &proto_label,
                            profile.params_b,
                            r,
                        ) {
                            Ok(()) => println!(
                                "    saved calibrated_ring {r} → {}/model_profiles.json",
                                args.profile_dir.display()
                            ),
                            Err(e) => eprintln!("    cannot persist calibrated_ring: {e}"),
                        }
                    }
                    None => println!(
                        "  → ring 0 is NOT solid — this model can't reliably drive even the core; \
                         pick a stronger model or recalibrate"
                    ),
                }
            }
            if let Some(path) = &args.report {
                let body = "# Ring Calibration\n\nPer-model recommended `--max-ring` is printed to \
                    stdout; the per-ring rows are in the sibling `.jsonl`.\n";
                write_outputs(path, body, rows)?;
            }
            return Ok::<(), String>(());
        }

        if let Some(models) = &args.models {
            // Fleet sweep: bench each model (same backend, model overridden) → leaderboard.
            let model_list: Vec<String> = models
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let mut summaries: Vec<BenchSummary> = Vec::new();
            for model in &model_list {
                println!("\n=== {model} ===");
                let mut opts = args.backend_opts.clone();
                opts.model = Some(model.clone());
                let provider_box = match create_provider(&opts).await {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("skip {model}: {e}");
                        continue;
                    }
                };
                let provider = provider_box.as_ref();
                let protocol =
                    select_protocol(&policy, &provider.capabilities(), protocol_override);
                summaries.push(
                    bench_model(
                        provider,
                        protocol,
                        &all_tools,
                        &schema,
                        args.iterations,
                        model,
                    )
                    .await?,
                );
            }
            if summaries.is_empty() {
                return Err("no models benched (all skipped)".to_string());
            }
            let leaderboard = render_leaderboard(&summaries);
            println!("\n{leaderboard}");
            if let Some(path) = &args.report {
                let rows: Vec<serde_json::Value> =
                    summaries.iter().flat_map(summary_rows).collect();
                write_outputs(path, &leaderboard, rows)?;
            }
        } else {
            // Single model (the per-tool diagnostic report).
            let provider_box = create_provider(&args.backend_opts).await?;
            let provider = provider_box.as_ref();
            let protocol = select_protocol(&policy, &provider.capabilities(), protocol_override);
            let model_label = args
                .backend_opts
                .model
                .clone()
                .unwrap_or_else(|| provider.id().to_string());
            println!(
                "Toolbench: {} tools x {} iterations | backend={} protocol={:?}",
                all_tools.len(),
                args.iterations,
                provider.id(),
                protocol,
            );
            let summary = bench_model(
                provider,
                protocol,
                &all_tools,
                &schema,
                args.iterations,
                &model_label,
            )
            .await?;
            let report = render_report(&summary);
            println!("\n{report}");
            if let Some(path) = &args.report {
                write_outputs(path, &report, summary_rows(&summary))?;
            }
        }

        Ok::<(), String>(())
    });

    if let Err(e) = result {
        eprintln!("toolbench error: {e}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(not(feature = "backend-openai"))]
pub fn run_toolbench(_args: ToolbenchArgs) -> ExitCode {
    eprintln!(
        "this binary was built without backend features; \
         rebuild with `cargo build --features backend-openai`"
    );
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_core::{Message, Role};
    use serde_json::json;

    fn text_completion(text: &str) -> Completion {
        Completion {
            message: Message::assistant(text),
            input_tokens: None,
            output_tokens: None,
            truncated: false,
        }
    }

    fn schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        })
    }

    fn native_completion(name: &str, args: serde_json::Value) -> Completion {
        Completion {
            message: Message {
                role: Role::Assistant,
                text: None,
                tool_calls: vec![ToolCall {
                    id: "t".to_string(),
                    name: name.to_string(),
                    args,
                }],
                tool_call_id: None,
                media: Vec::new(),
            },
            input_tokens: None,
            output_tokens: None,
            truncated: false,
        }
    }

    #[test]
    fn outcome_is_success() {
        assert!(Outcome::Success.is_success());
        assert!(!Outcome::NoAction.is_success());
        assert!(!Outcome::WrongTool("x".to_string()).is_success());
    }

    #[test]
    fn classify_success_native() {
        let c = native_completion("read_file", json!({"path": "x"}));
        assert_eq!(
            classify(ActionProtocol::NativeTools, &c, "read_file", &schema()),
            Outcome::Success
        );
    }

    #[test]
    fn classify_success_constrained() {
        let c = text_completion(r#"{"tool":"read_file","args":{"path":"x"}}"#);
        assert_eq!(
            classify(ActionProtocol::ConstrainedJson, &c, "read_file", &schema()),
            Outcome::Success
        );
    }

    #[test]
    fn classify_wrong_tool() {
        let c = native_completion("write_file", json!({"path": "x"}));
        assert_eq!(
            classify(ActionProtocol::NativeTools, &c, "read_file", &schema()),
            Outcome::WrongTool("write_file".to_string())
        );
    }

    #[test]
    fn classify_malformed_args() {
        // Right tool, but the required "path" arg is missing.
        let c = native_completion("read_file", json!({}));
        assert_eq!(
            classify(ActionProtocol::NativeTools, &c, "read_file", &schema()),
            Outcome::MalformedArgs
        );
    }

    #[test]
    fn classify_no_action() {
        // Native: tool_calls empty (the model chatted instead of calling).
        let c = text_completion("I cannot help with that.");
        assert_eq!(
            classify(ActionProtocol::NativeTools, &c, "read_file", &schema()),
            Outcome::NoAction
        );
    }

    #[test]
    fn classify_parse_error() {
        // ConstrainedJson: non-empty text that is not a valid action object.
        let bad_json = text_completion(r#"{"tool": "read_file", "args": {oops"#);
        assert_eq!(
            classify(
                ActionProtocol::ConstrainedJson,
                &bad_json,
                "read_file",
                &schema()
            ),
            Outcome::ParseError
        );
        // TextXml: prose with no <tool_call> is a parse failure too.
        let prose = text_completion("just some prose");
        assert_eq!(
            classify(ActionProtocol::TextXml, &prose, "read_file", &schema()),
            Outcome::ParseError
        );
    }

    #[test]
    fn outcome_labels() {
        assert_eq!(Outcome::Success.label(), "success");
        assert_eq!(Outcome::WrongTool("x".to_string()).label(), "wrong_tool");
        assert_eq!(Outcome::MalformedArgs.label(), "malformed_args");
        assert_eq!(Outcome::NoAction.label(), "no_action");
        assert_eq!(Outcome::ParseError.label(), "parse_error");
    }

    #[test]
    fn verdict_bands() {
        assert_eq!(verdict(0.90), "solid");
        assert_eq!(verdict(0.89), "marginal");
        assert_eq!(verdict(0.70), "marginal");
        assert_eq!(verdict(0.69), "unreliable");
    }

    fn sample_summary() -> BenchSummary {
        BenchSummary {
            model: "qwen2.5-coder:7b".to_string(),
            backend: "openai-http".to_string(),
            protocol: "ConstrainedJson".to_string(),
            iterations: 10,
            per_tool: vec![
                ToolStat {
                    name: "read_file".to_string(),
                    fires: 10,
                    success: 9,
                    histogram: vec![("no_action".to_string(), 1), ("success".to_string(), 9)],
                },
                ToolStat {
                    name: "write_file".to_string(),
                    fires: 10,
                    success: 5,
                    histogram: vec![
                        ("malformed_args".to_string(), 5),
                        ("success".to_string(), 5),
                    ],
                },
            ],
        }
    }

    #[test]
    fn render_report_has_taxonomy_and_verdict() {
        let md = render_report(&sample_summary());
        assert!(md.contains("read_file") && md.contains("write_file"));
        assert!(md.contains("90.0%")); // read_file rate
        assert!(md.contains("no_action×1")); // failure taxonomy in-table
        assert!(md.contains("malformed_args×5"));
        assert!(md.contains("Overall:"));
        // overall 14/20 = 70% → marginal; read_file 90% solid; write_file 50% unreliable.
        assert!(md.contains("marginal") && md.contains("solid") && md.contains("unreliable"));
    }

    #[test]
    fn summary_rows_shape() {
        let rows = summary_rows(&sample_summary());
        assert_eq!(rows.len(), 3); // 2 tools + overall
        assert_eq!(rows[0]["model"], "qwen2.5-coder:7b");
        assert_eq!(rows[0]["tool"], "read_file");
        assert_eq!(rows[0]["fires"], 10);
        assert_eq!(rows[0]["success"], 9);
        assert_eq!(rows[0]["histogram"]["no_action"], 1);
        let overall = rows.last().unwrap();
        assert_eq!(overall["tool"], "__overall__");
        assert_eq!(overall["model"], "qwen2.5-coder:7b");
        assert_eq!(overall["success"], 14);
        assert_eq!(overall["fires"], 20);
        assert_eq!(overall["verdict"], "marginal");
    }

    fn summary_with(model: &str, success: u32, fires: u32) -> BenchSummary {
        BenchSummary {
            model: model.to_string(),
            backend: "openai-http".to_string(),
            protocol: "ConstrainedJson".to_string(),
            iterations: fires,
            per_tool: vec![ToolStat {
                name: "read_file".to_string(),
                fires,
                success,
                histogram: vec![("success".to_string(), success)],
            }],
        }
    }

    #[test]
    fn recommend_max_ring_longest_solid_prefix() {
        assert_eq!(recommend_max_ring(&[true, true]), Some(1));
        assert_eq!(recommend_max_ring(&[true, false]), Some(0));
        assert_eq!(recommend_max_ring(&[false, true]), None);
        assert_eq!(recommend_max_ring(&[]), None);
        assert_eq!(recommend_max_ring(&[true, true, true]), Some(2));
        // A later solid ring after a break does not count (unbroken prefix only).
        assert_eq!(recommend_max_ring(&[true, true, false, true]), Some(1));
    }

    #[test]
    fn leaderboard_sorts_best_first() {
        let weak = summary_with("tiny-0.5b", 3, 10); // 30% unreliable
        let strong = summary_with("qwen-7b", 10, 10); // 100% solid
        let mid = summary_with("phi-mini", 8, 10); // 80% marginal
        let board = render_leaderboard(&[weak, strong, mid]);
        let p_strong = board.find("qwen-7b").expect("strong model in board");
        let p_mid = board.find("phi-mini").expect("mid model in board");
        let p_weak = board.find("tiny-0.5b").expect("weak model in board");
        assert!(
            p_strong < p_mid && p_mid < p_weak,
            "leaderboard not sorted best→worst:\n{board}"
        );
        assert!(
            board.contains("solid") && board.contains("marginal") && board.contains("unreliable")
        );
        assert!(board.contains("100.0%"));
    }
}

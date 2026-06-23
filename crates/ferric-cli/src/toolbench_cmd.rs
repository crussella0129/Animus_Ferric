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
#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai", test))]
use ferric_core::{ActionProtocol, ToolCall};
#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai", test))]
use ferric_provider::Completion;

#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai"))]
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

    /// Write a Markdown report here (+ a sibling `.jsonl`). Without it, the
    /// report only prints to stdout.
    #[arg(long)]
    pub report: Option<PathBuf>,
}

/// The classified outcome of one toolbench iteration. This is what turns the
/// bench from a pass/fail counter into a diagnostic: it says *why* a model
/// missed, so a user can judge whether a smaller model is still good enough.
#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai", test))]
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

#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai", test))]
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
#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai", test))]
pub fn classify(
    protocol: ActionProtocol,
    completion: &Completion,
    target: &str,
    schema: &serde_json::Value,
) -> Outcome {
    let text = completion.message.text.as_deref().unwrap_or_default();
    let parsed: Result<Option<ToolCall>, ()> = match protocol {
        ActionProtocol::NativeTools => Ok(completion.message.tool_calls.first().cloned()),
        ActionProtocol::ConstrainedJson => {
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
#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai", test))]
pub struct ToolStat {
    pub name: String,
    pub fires: u32,
    pub success: u32,
    pub histogram: Vec<(String, u32)>,
}

#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai", test))]
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
#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai", test))]
pub struct BenchSummary {
    pub backend: String,
    pub protocol: String,
    pub iterations: u32,
    pub per_tool: Vec<ToolStat>,
}

#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai", test))]
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
#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai", test))]
pub fn verdict(rate: f64) -> &'static str {
    if rate >= 0.90 {
        "solid"
    } else if rate >= 0.70 {
        "marginal"
    } else {
        "unreliable"
    }
}

/// Render the human-facing Markdown report.
#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai", test))]
pub fn render_report(s: &BenchSummary) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "# Toolbench Report\n");
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
#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai", test))]
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

#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai"))]
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
    };
    let user_prompt =
        format!("Invoke the `{tool_name}` tool with dummy data that matches its schema.");
    // ADR-010: constraint XOR tools, by construction.
    let (tools, constraint) = match protocol {
        ActionProtocol::NativeTools => (all_tools.to_vec(), None),
        ActionProtocol::ConstrainedJson => {
            (Vec::new(), Some(Constraint::JsonSchema(schema.clone())))
        }
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

#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai"))]
pub fn run_toolbench(args: ToolbenchArgs) -> ExitCode {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("tokio runtime error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let result = runtime.block_on(async {
        let provider_box = create_provider(&args.backend_opts).await?;
        let provider = provider_box.as_ref();

        let mut registry = Registry::new();
        ferric_tools::register_builtin_tools(&mut registry);

        let profile = ModelProfile {
            params_b: 8.0,
            quant: "Q4".to_string(),
            ctx: 4096,
            family: "unknown".to_string(),
            measured_level: None,
        };
        let policy = policy_for(&profile);

        let all_tools: Vec<ToolDescriptor> = registry
            .tools_for_policy(&policy)
            .into_iter()
            .map(|spec| ToolDescriptor {
                name: spec.name,
                description: spec.description,
                input_schema: spec.input_schema,
            })
            .collect();

        // Protocol from the backend's real capabilities (an explicit
        // `--protocol` overrides), so the bench measures what `ferric query`
        // would actually run against this backend.
        let protocol = select_protocol(
            &policy,
            &provider.capabilities(),
            args.protocol.map(ActionProtocol::from),
        );
        let schema = action_schema(&all_tools);

        println!(
            "Toolbench: {} tools x {} iterations | backend={} protocol={:?}",
            all_tools.len(),
            args.iterations,
            provider.id(),
            protocol,
        );

        let mut per_tool: Vec<ToolStat> = Vec::new();

        for tool in &all_tools {
            use std::collections::BTreeMap;
            let mut hist: BTreeMap<&'static str, u32> = BTreeMap::new();
            print!("Testing tool '{:<15}': ", tool.name);

            for _ in 0..args.iterations {
                let request = build_request(protocol, &all_tools, &schema, &tool.name);
                let completion = provider
                    .complete(request)
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
                fires: args.iterations,
                success,
                histogram,
            };
            println!(
                " [{} / {}] ({:.1}%) {}",
                success,
                args.iterations,
                stat.rate() * 100.0,
                verdict(stat.rate()),
            );
            per_tool.push(stat);
        }

        let summary = BenchSummary {
            backend: provider.id().to_string(),
            protocol: format!("{protocol:?}"),
            iterations: args.iterations,
            per_tool,
        };

        let report = render_report(&summary);
        println!("\n{report}");

        if let Some(path) = &args.report {
            std::fs::write(path, &report)
                .map_err(|e| format!("write report {}: {e}", path.display()))?;
            let jsonl: String = summary_rows(&summary)
                .iter()
                .map(|r| r.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            let jsonl_path = path.with_extension("jsonl");
            std::fs::write(&jsonl_path, jsonl)
                .map_err(|e| format!("write {}: {e}", jsonl_path.display()))?;
            println!("Wrote {} + {}", path.display(), jsonl_path.display());
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

#[cfg(not(any(feature = "backend-mistralrs", feature = "backend-openai")))]
pub fn run_toolbench(_args: ToolbenchArgs) -> ExitCode {
    eprintln!(
        "this binary was built without backend features; \
         rebuild with `cargo build --features backend-mistralrs,backend-openai`"
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
        assert_eq!(rows[0]["tool"], "read_file");
        assert_eq!(rows[0]["fires"], 10);
        assert_eq!(rows[0]["success"], 9);
        assert_eq!(rows[0]["histogram"]["no_action"], 1);
        let overall = rows.last().unwrap();
        assert_eq!(overall["tool"], "__overall__");
        assert_eq!(overall["success"], 14);
        assert_eq!(overall["fires"], 20);
        assert_eq!(overall["verdict"], "marginal");
    }
}

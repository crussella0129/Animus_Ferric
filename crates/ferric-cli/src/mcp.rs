//! `ferric mcp` — the MCP-stdio server (ADR-046, the ADR-005 security call for
//! ADR-012). JSON-RPC 2.0 over newline-delimited stdin/stdout: stdout carries
//! protocol frames ONLY (never a log line — that would corrupt the stream);
//! all diagnostics go to stderr. Workspace/backend/model/protocol are fixed at
//! launch (`McpArgs`); the exposed tool schema carries none of them, so the
//! containment guarantee is structural, not something a handler enforces.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::media::Modality;
use ferric_guard::Workspace;
use ferric_loop::RunArgs;
use ferric_trace::JsonlSink;

use crate::backend::ResolvedBackend;
use crate::config::RunFlags;
use crate::query::{
    RunConfig, build_run_config, mock_provider, open_workspace, route_files, run_with_provider,
};

/// One incoming line, parsed. A request expects a response (`id` present,
/// even if `null`); a notification has no `id` field at all and expects none.
#[derive(Debug, Clone, Deserialize)]
pub struct RpcRequest {
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl RpcRequest {
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RpcNotification {
    pub jsonrpc: &'static str,
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

pub const PARSE_ERROR: i64 = -32700;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;

impl RpcResponse {
    pub fn success(id: Value, result: Value) -> Self {
        RpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Value, code: i64, message: impl Into<String>) -> Self {
        RpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

/// Parse one input line. On success, the caller distinguishes a request from
/// a notification via `is_notification()`. On failure, a `-32700 Parse error`
/// response is returned with `id: null` (no id could be recovered from
/// unparseable input, per the JSON-RPC 2.0 spec).
pub fn parse_line(line: &str) -> Result<RpcRequest, Box<RpcResponse>> {
    serde_json::from_str::<RpcRequest>(line).map_err(|e| {
        Box::new(RpcResponse::error(
            Value::Null,
            PARSE_ERROR,
            format!("parse error: {e}"),
        ))
    })
}

/// Serialize a response as a single line (no embedded newline) for stdout.
pub fn render_line(resp: &RpcResponse) -> String {
    serde_json::to_string(resp).unwrap_or_else(|_| {
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"internal error serializing response"}}"#
            .to_string()
    })
}

/// Fixed, hardcoded protocol version (ADR-005 spirit: no runtime negotiation
/// surface). A client requesting a different version gets this one back and
/// decides for itself whether to proceed.
pub const PROTOCOL_VERSION: &str = "2025-11-25";

pub fn handle_initialize(id: Value) -> RpcResponse {
    RpcResponse::success(
        id,
        serde_json::json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {"tools": {}, "prompts": {}},
            "serverInfo": {"name": "ferric", "version": env!("CARGO_PKG_VERSION")},
        }),
    )
}

/// The one MCP tool this server exposes. Deliberately has NO `workspace`,
/// `backend`, or `model` field — those are `ferric mcp` launch-time CLI flags,
/// fixed for the server's lifetime (ADR-046: the containment guarantee is
/// structural, not something a handler must remember to enforce).
pub fn ferric_query_tool_schema() -> Value {
    serde_json::json!({
        "name": "ferric_query",
        "description": "Run a one-shot, workspace-scoped, policy-scaled coding/agentic \
            task against the local model this Ferric MCP server was launched with. The \
            full constrained agent loop runs (planning, tool calls, guard-enforced \
            permission checks) inside Ferric's own workspace boundary; only the final \
            answer is returned.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The task to perform, or an optional goal amendment for a non-clarification continuation.",
                },
                "files": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional paths to attach. Text/code folds into the \
                        prompt; images/audio/video attach as media if the server's \
                        declared --modality supports it.",
                },
                "continuation_id": {
                    "type": "string",
                    "description": "Opaque id returned by an incomplete prior ferric_query call. Never a trace path.",
                },
                "answer": {
                    "type": "string",
                    "description": "Answer to a pending structured clarification. Use with continuation_id and without prompt/files.",
                },
            },
            "additionalProperties": false,
            "anyOf": [
                {"required": ["prompt"]},
                {"required": ["continuation_id"]},
                {"required": ["answer"]}
            ]
        },
        "outputSchema": {
            "type": "object",
            "properties": {
                "status": {"type": "string", "enum": ["completed", "needs_input", "failed"]},
                "text": {"type": ["string", "null"]},
                "turns": {"type": "integer", "minimum": 0},
                "stop_reason": {"type": "string"},
                "continuation_id": {"type": ["string", "null"]},
                "needs_input": {"type": ["object", "null"]}
            },
            "required": ["status", "text", "turns", "stop_reason", "continuation_id", "needs_input"],
            "additionalProperties": false
        }
    })
}

pub fn handle_tools_list(id: Value) -> RpcResponse {
    RpcResponse::success(
        id,
        serde_json::json!({"tools": [ferric_query_tool_schema()]}),
    )
}

/// `ferric mcp`'s CLI surface: `QueryArgs` minus `prompt`/`files` — those
/// become the per-`tools/call` `prompt`/`files` arguments instead. Everything
/// here is launch-time-fixed for the server's lifetime (ADR-046).
#[derive(Args)]
pub struct McpArgs {
    #[command(flatten)]
    pub run: RunFlags,

    /// Declare the model's accepted non-text modalities (comma list:
    /// `image,audio,video`). Applies to every `tools/call`'s `files`.
    #[arg(long)]
    pub modality: Option<String>,

    /// Resume an interrupted, still-incomplete session by replaying its
    /// trace (sprint 55). Same semantics as `ferric query --resume`: the
    /// first `ferric_query` call to this server will continue the task.
    #[arg(long)]
    pub resume: Option<PathBuf>,
}

/// The long-lived state one `ferric mcp` process holds: workspace/backend/
/// model/protocol are fixed at launch (`RunConfig` and `Workspace`). A real
/// provider and its ONE runtime are built at launch and reused for every
/// `tools/call` (T-3601); the finite scripted mock is reset per request.
pub(crate) struct McpServer {
    pub workspace: Workspace,
    pub config: RunConfig,
    pub backend: ResolvedBackend,
    pub declared: Vec<Modality>,
    pub trace_dir: PathBuf,
    pub resume: std::sync::Mutex<Option<ferric_loop::ReplayedState>>,
}

fn tool_content(text: String, is_error: bool) -> Value {
    serde_json::json!({
        "content": [{"type": "text", "text": text}],
        "isError": is_error,
    })
}

fn outcome_content(outcome: &ferric_loop::LoopOutcome, continuation_id: Option<&str>) -> Value {
    let status = if outcome.stop.is_success() {
        "completed"
    } else if outcome.needs_input.is_some() {
        "needs_input"
    } else {
        "failed"
    };
    let legacy_text = if let Some(needs_input) = &outcome.needs_input {
        let options = if needs_input.request.options.is_empty() {
            String::new()
        } else {
            format!("\nOptions: {}", needs_input.request.options.join(", "))
        };
        format!(
            "Ferric needs input before continuing.\nQuestion: {}\nContext: {}{}\nContinuation: {}",
            needs_input.request.question,
            needs_input.request.context,
            options,
            continuation_id.unwrap_or(&needs_input.continuation_id)
        )
    } else {
        outcome.final_text.clone().unwrap_or_else(|| {
            format!("query stopped before completion: {}", outcome.stop.as_str())
        })
    };
    let structured = serde_json::json!({
        "status": status,
        "text": outcome.final_text,
        "turns": outcome.turns,
        "stop_reason": outcome.stop.as_str(),
        "continuation_id": continuation_id,
        "needs_input": outcome.needs_input,
    });
    serde_json::json!({
        "content": [{"type": "text", "text": legacy_text}],
        "structuredContent": structured,
        "isError": status == "failed",
    })
}

fn optional_string_argument(arguments: &Value, name: &str) -> Result<Option<String>, String> {
    match arguments.get(name) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(format!("\"{name}\" must be a string")),
    }
}

impl McpServer {
    /// Dispatch one parsed request. Returns `None` for notifications (no
    /// response is ever sent for those, per the JSON-RPC 2.0 spec).
    pub fn dispatch(&self, req: RpcRequest) -> Option<RpcResponse> {
        if req.is_notification() {
            return None;
        }
        let id = req.id.clone().unwrap_or(Value::Null);
        Some(match req.method.as_str() {
            "initialize" => handle_initialize(id),
            "tools/list" => handle_tools_list(id),
            "tools/call" => self.handle_tools_call(id, &req.params),
            "prompts/list" => self.handle_prompts_list(id),
            "prompts/get" => self.handle_prompts_get(id, &req.params),
            other => RpcResponse::error(id, METHOD_NOT_FOUND, format!("unknown method: {other}")),
        })
    }

    /// Skills offered as MCP prompts.
    ///
    /// Listing is safe to do unconditionally: an MCP prompt is *pulled* by the
    /// client, so surfacing one is not the harness deciding to run it. The
    /// authorization gate that `--skill` and the config allowlist provide
    /// (ADR-091) governs the other direction — skills folded into a `query`
    /// prompt by Ferric itself.
    ///
    /// Parsing goes through `ferric-skills` rather than a second local parser.
    /// The one this replaced accepted a `SKILL.md` with no frontmatter at all
    /// (falling back to the directory name) and let a declared `name` override
    /// its directory — so a skill installed as `helpful/` could answer to
    /// `deploy`. Names are now required to match their directory.
    fn handle_prompts_list(&self, id: Value) -> RpcResponse {
        let (skills, _errors) = crate::skills::discover(self.workspace.root());
        let prompts: Vec<Value> = skills
            .into_iter()
            .map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "description": s.description,
                    "arguments": []
                })
            })
            .collect();
        RpcResponse::success(id, serde_json::json!({ "prompts": prompts }))
    }

    fn handle_prompts_get(&self, id: Value, params: &Value) -> RpcResponse {
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        let (skills, _errors) = crate::skills::discover(self.workspace.root());

        if let Some(skill) = skills.into_iter().find(|s| s.name == name) {
            RpcResponse::success(
                id,
                serde_json::json!({
                    "description": skill.description,
                    "messages": [
                        {
                            "role": "user",
                            "content": {
                                "type": "text",
                                "text": skill.instructions
                            }
                        }
                    ]
                }),
            )
        } else {
            RpcResponse::error(id, INVALID_PARAMS, format!("unknown prompt: {name}"))
        }
    }

    fn handle_tools_call(&self, id: Value, params: &Value) -> RpcResponse {
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        if name != "ferric_query" {
            return RpcResponse::error(id, INVALID_PARAMS, format!("unknown tool: {name}"));
        }
        let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
        let prompt = match optional_string_argument(&arguments, "prompt") {
            Ok(value) => value,
            Err(message) => return RpcResponse::error(id, INVALID_PARAMS, message),
        };
        let continuation_id = match optional_string_argument(&arguments, "continuation_id") {
            Ok(value) => value,
            Err(message) => return RpcResponse::error(id, INVALID_PARAMS, message),
        };
        let answer = match optional_string_argument(&arguments, "answer") {
            Ok(value) => value,
            Err(message) => return RpcResponse::error(id, INVALID_PARAMS, message),
        };
        let files: Vec<PathBuf> = match arguments.get("files") {
            None => Vec::new(),
            Some(Value::Array(values)) => {
                let mut files = Vec::with_capacity(values.len());
                for value in values {
                    let Some(path) = value.as_str() else {
                        return RpcResponse::error(
                            id,
                            INVALID_PARAMS,
                            "every \"files\" item must be a string path",
                        );
                    };
                    files.push(PathBuf::from(path));
                }
                files
            }
            Some(_) => {
                return RpcResponse::error(id, INVALID_PARAMS, "\"files\" must be an array");
            }
        };

        let mut resume_slot = self.resume.lock().unwrap();
        let resume_ref = resume_slot.as_ref();
        if let (Some(expected), Some(actual)) = (
            resume_ref.map(|state| state.source_session.as_str()),
            continuation_id.as_deref(),
        ) && expected != actual
        {
            return RpcResponse::error(
                id,
                INVALID_PARAMS,
                format!("unknown continuation_id {actual:?}"),
            );
        }

        match resume_ref {
            None => {
                if continuation_id.is_some()
                    || answer.is_some()
                    || prompt
                        .as_deref()
                        .is_none_or(|value| value.trim().is_empty())
                {
                    return RpcResponse::error(
                        id,
                        INVALID_PARAMS,
                        "a new ferric_query call requires one non-empty prompt",
                    );
                }
            }
            Some(state) if state.core.pending_input.is_some() => {
                if prompt.is_some()
                    || !files.is_empty()
                    || answer
                        .as_deref()
                        .is_none_or(|value| value.trim().is_empty())
                {
                    return RpcResponse::error(
                        id,
                        INVALID_PARAMS,
                        "a clarification continuation requires a non-empty answer and accepts no prompt or files",
                    );
                }
            }
            Some(_) => {
                if answer.is_some()
                    || prompt
                        .as_deref()
                        .is_some_and(|value| value.trim().is_empty())
                {
                    return RpcResponse::error(
                        id,
                        INVALID_PARAMS,
                        "a non-clarification continuation accepts an optional non-empty prompt, not answer",
                    );
                }
            }
        }
        let resume_state = resume_slot.take();
        drop(resume_slot);

        let (media_parts, effective_prompt) = if answer.is_some() {
            (Vec::new(), None)
        } else {
            let (media_parts, prompt_suffix) = match route_files(
                &self.workspace,
                &files,
                &self.declared,
                self.config.caps.supports_media,
            ) {
                Ok(routed) => routed,
                Err(e) => {
                    if let Some(state) = resume_state {
                        *self.resume.lock().unwrap() = Some(state);
                    }
                    return RpcResponse::success(id, tool_content(format!("files: {e}"), true));
                }
            };
            (
                media_parts,
                crate::query::with_attachments(prompt, prompt_suffix),
            )
        };

        let (session, trace_path, mut sink) =
            match crate::query::create_trace_sink(&self.trace_dir, "mcp") {
                Ok(trace) => trace,
                Err(e) => {
                    if let Some(state) = resume_state {
                        *self.resume.lock().unwrap() = Some(state);
                    }
                    return RpcResponse::success(
                        id,
                        tool_content(format!("cannot open trace: {e}"), true),
                    );
                }
            };

        match self.run_one(
            &mut sink,
            effective_prompt.as_deref(),
            media_parts,
            resume_state.clone(),
            answer.as_deref(),
        ) {
            Ok(outcome) => {
                let continuation = if outcome.stop.is_success() {
                    None
                } else {
                    match ferric_loop::replay(&trace_path) {
                        Ok(state) => {
                            *self.resume.lock().unwrap() = Some(state);
                            Some(session.as_str())
                        }
                        Err(error) => {
                            return RpcResponse::success(
                                id,
                                tool_content(
                                    format!("cannot retain continuation state: {error}"),
                                    true,
                                ),
                            );
                        }
                    }
                };
                RpcResponse::success(id, outcome_content(&outcome, continuation))
            }
            Err(message) => {
                if let Some(state) = resume_state {
                    *self.resume.lock().unwrap() = Some(state);
                }
                RpcResponse::success(id, tool_content(message, true))
            }
        }
    }

    fn run_one(
        &self,
        sink: &mut JsonlSink,
        prompt: Option<&str>,
        media: Vec<ferric_core::MediaPart>,
        resume: Option<ferric_loop::ReplayedState>,
        answer: Option<&str>,
    ) -> Result<ferric_loop::LoopOutcome, String> {
        // MCP streams partial text via custom JSON-RPC notification.
        let sink_fn = |d: ferric_provider::StreamDelta| match d {
            ferric_provider::StreamDelta::Text(t) | ferric_provider::StreamDelta::Thought(t) => {
                let note = RpcNotification {
                    jsonrpc: "2.0",
                    method: "ferric/streamDelta".to_string(),
                    params: serde_json::json!({ "text": t }),
                };
                if let Ok(line) = serde_json::to_string(&note) {
                    let mut stdout = std::io::stdout().lock();
                    let _ = std::io::Write::write_all(&mut stdout, line.as_bytes());
                    let _ = std::io::Write::write_all(&mut stdout, b"\n");
                    let _ = std::io::Write::flush(&mut stdout);
                }
            }
            _ => {}
        };

        self.backend.with_provider(
            || mock_provider(self.config.protocol),
            |provider| {
                self.backend.block_on(run_with_provider(
                    RunArgs {
                        media,
                        stream_sink: Some(&sink_fn),
                        resume,
                        answer,
                        ..self.config.run_args(&self.workspace, provider)
                    },
                    sink,
                    prompt,
                ))
            },
        )
    }

    /// Build the server: workspace + launch-time-fixed run config. A real
    /// backend and its `tokio::runtime::Runtime` are constructed exactly once;
    /// the finite scripted mock is reconstructed for each `tools/call`.
    pub fn launch(args: &McpArgs) -> Result<Self, String> {
        let (workspace_root, workspace) = open_workspace(args.run.workspace.as_deref())?;

        // T-3805: layered config (CLI flag > project `.ferric/config.toml` >
        // user config > today's hardcoded default). The merged backend options
        // are what reach the real provider below, not just protocol/tier
        // selection.
        let cfg = crate::config::load_layered(&workspace_root)
            .map_err(|error| error.to_string())?
            .config;
        // `ferric_query` over MCP does not fold skills. The MCP client already
        // reaches skills the right way — as *prompts* it pulls deliberately
        // (`prompts/get`). Injecting them into the tool call as well would run
        // a skill the client never asked for, which is exactly the automatic
        // invocation ADR-091 rules out.
        let run = args.run.resolve(&cfg, &workspace_root);
        let mut config = build_run_config(&run).map_err(|error| error.to_string())?;
        // T-3905 (sprint 39 / 55): `--resume <path>` replays an interrupted, still-
        // incomplete session. Resolved here and passed to the first `ferric_query` call.
        let resume_state = args
            .resume
            .as_deref()
            .map(|path| crate::query::load_resume(path, workspace.root(), &config))
            .transpose()?;

        // No trace sink exists yet at launch (each tools/call opens its own),
        // so a composition failure is surfaced once here rather than dropped, or (unlike
        // `Note`-tracing) repeated on every subsequent call.
        if let Some(err) = &config.prompt_composition_error {
            eprintln!("{err}");
        }

        // T-3806: `Animus.md` — same fold as `run_query`, its presence
        // surfaced here for the same reason as the diagnostic above.
        let animus_md = crate::query::fold_workspace_animus_md(&mut config, &workspace_root);
        if let Some(chars) = animus_md {
            eprintln!("Animus.md applied ({chars} chars)");
        }
        crate::query::note_frozen_system_prompt(
            &config,
            resume_state.is_some(),
            animus_md.is_some(),
        );

        let trace_dir = ferric_trace::trace_dir(&workspace_root);
        std::fs::create_dir_all(&trace_dir)
            .map_err(|e| format!("cannot create trace dir {}: {e}", trace_dir.display()))?;
        let declared = crate::media::parse_modalities(args.modality.as_deref().unwrap_or(""));
        let backend = ResolvedBackend::resolve(run.mock, &run.backend, workspace.root())?;

        Ok(McpServer {
            workspace,
            config,
            backend,
            declared,
            trace_dir,
            resume: std::sync::Mutex::new(resume_state),
        })
    }

    /// Serve JSON-RPC requests from stdin until EOF, writing responses to
    /// stdout (newline-delimited, protocol frames only — see the module doc).
    pub fn serve(&self) {
        let stdin = std::io::stdin();
        let mut stdout = std::io::stdout();
        for line in stdin.lock().lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if line.trim().is_empty() {
                continue;
            }
            let response = match parse_line(&line) {
                Ok(req) => self.dispatch(req),
                Err(parse_error) => Some(*parse_error),
            };
            if let Some(response) = response {
                let _ = writeln!(stdout, "{}", render_line(&response));
                let _ = stdout.flush();
            }
        }
    }
}

/// `ferric mcp` entrypoint.
pub fn run_mcp(args: McpArgs) -> std::process::ExitCode {
    match McpServer::launch(&args) {
        Ok(server) => {
            server.serve();
            std::process::ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HarnessPolicyArg;
    use ferric_provider::Provider;

    /// All-`None`/`mock: true` `McpArgs` for a given workspace.
    fn base_mcp_args(ws: &std::path::Path) -> McpArgs {
        McpArgs {
            run: RunFlags {
                workspace: Some(ws.to_path_buf()),
                mock: true,
                ..RunFlags::default()
            },
            modality: None,
            resume: None,
        }
    }

    #[test]
    fn bounded_mcp_request_preserves_explicit_evidence_policy() {
        let dir = tempfile::tempdir().unwrap();
        let mut args = base_mcp_args(dir.path());
        args.run.harness_policy = Some(HarnessPolicyArg::Evidence);
        let server = McpServer::launch(&args).unwrap();
        assert_eq!(
            server.config.harness_policy,
            Some(ferric_core::HarnessPolicy::Evidence)
        );

        let response = server
            .dispatch(call_request(
                1,
                serde_json::json!({"prompt": "complete one bounded mock task"}),
            ))
            .unwrap();
        assert_eq!(response.result.as_ref().unwrap()["isError"], false);

        let trace = std::fs::read_dir(&server.trace_dir)
            .unwrap()
            .next()
            .expect("one bounded MCP request trace")
            .unwrap()
            .path();
        let selected = ferric_trace::TraceReader::open(trace)
            .unwrap()
            .find_map(|record| match record.unwrap().event {
                ferric_trace::ParsedEvent::Known(ferric_trace::Event::PolicySelected {
                    harness_policy,
                    ..
                }) => Some(harness_policy),
                _ => None,
            })
            .expect("a policy_selected event");
        assert_eq!(selected, ferric_core::HarnessPolicy::Evidence);
    }

    #[test]
    fn unsupported_planner_fails_before_mcp_trace_or_provider() {
        let dir = tempfile::tempdir().unwrap();
        let mut args = base_mcp_args(dir.path());
        args.run.harness_policy = Some(HarnessPolicyArg::EvidencePlanner);

        let error = McpServer::launch(&args)
            .err()
            .expect("planner is unavailable");
        assert!(error.contains("evidence_planner"), "{error}");
        assert!(!dir.path().join(".ferric/trace").exists());
    }

    #[test]
    fn parses_valid_request_line() {
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
        let req = parse_line(line).unwrap();
        assert_eq!(req.method, "tools/list");
        assert_eq!(req.id, Some(Value::from(1)));
        assert!(!req.is_notification());
    }

    #[test]
    fn parses_notification_without_id() {
        let line = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let req = parse_line(line).unwrap();
        assert!(req.is_notification());
    }

    #[test]
    fn malformed_line_yields_parse_error() {
        let resp = parse_line("not json at all").unwrap_err();
        assert_eq!(resp.error.as_ref().unwrap().code, PARSE_ERROR);
        assert_eq!(resp.id, Value::Null);
    }

    /// test-critique C-004: a source-level guard that this module never
    /// writes a bare `println!` — stdout is reserved for JSON-RPC frames
    /// (module doc comment); a stray log line would corrupt the stream. Cheap
    /// static check standing in for a runtime stdio-capture test.
    #[test]
    fn no_bare_println_in_source() {
        let source = include_str!("mcp.rs");
        for line in source.lines() {
            let trimmed = line.trim_start();
            assert!(
                !trimmed.starts_with("println!"),
                "found a bare println! in mcp.rs (must be eprintln!): {trimmed}"
            );
        }
    }

    #[test]
    fn render_line_has_no_embedded_newline() {
        let resp = RpcResponse::success(Value::from(1), serde_json::json!({"ok": true}));
        let line = render_line(&resp);
        assert!(!line.contains('\n'));
        assert!(line.contains("\"ok\":true"));
    }

    #[test]
    fn initialize_returns_fixed_version_and_tools_capability() {
        let resp = handle_initialize(Value::from(1));
        let result = resp.result.unwrap();
        assert_eq!(result["protocolVersion"], PROTOCOL_VERSION);
        assert!(result["capabilities"]["tools"].is_object());
        assert_eq!(result["serverInfo"]["name"], "ferric");
    }

    #[test]
    fn tools_list_has_exactly_one_tool_named_ferric_query() {
        let resp = handle_tools_list(Value::from(1));
        let tools = resp.result.unwrap()["tools"].as_array().unwrap().clone();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "ferric_query");
    }

    /// The structural containment-guarantee regression test (ADR-046): the
    /// exposed tool's schema must never grow a workspace/backend/model field,
    /// since that would let a caller redirect containment per-call instead of
    /// it being fixed at `ferric mcp` launch.
    #[test]
    fn ferric_query_schema_has_no_workspace_backend_or_model_field() {
        let schema = ferric_query_tool_schema();
        let properties = &schema["inputSchema"]["properties"];
        assert!(properties.get("workspace").is_none());
        assert!(properties.get("backend").is_none());
        assert!(properties.get("model").is_none());
        assert!(properties.get("prompt").is_some());
        assert!(properties.get("continuation_id").is_some());
        assert!(properties.get("answer").is_some());
        assert_eq!(
            schema["outputSchema"]["properties"]["status"]["type"],
            "string"
        );
    }

    fn test_server(dir: &std::path::Path, provider: Box<dyn Provider + Send + Sync>) -> McpServer {
        let trace_dir = dir.join(".ferric").join("trace");
        std::fs::create_dir_all(&trace_dir).unwrap();
        McpServer {
            workspace: Workspace::new(dir).unwrap(),
            config: build_run_config(&crate::query::RunConfigArgs::mock()).unwrap(),
            backend: ResolvedBackend::Real {
                provider,
                runtime: tokio::runtime::Runtime::new().unwrap(),
            },
            declared: Vec::new(),
            trace_dir,
            resume: std::sync::Mutex::new(None),
        }
    }

    fn call_request(id: i64, arguments: Value) -> RpcRequest {
        RpcRequest {
            id: Some(Value::from(id)),
            method: "tools/call".to_string(),
            params: serde_json::json!({"name": "ferric_query", "arguments": arguments}),
        }
    }

    fn native_completion(id: &str, name: &str, args: Value) -> ferric_provider::Completion {
        ferric_provider::Completion {
            message: ferric_core::Message {
                role: ferric_core::Role::Assistant,
                text: None,
                tool_calls: vec![ferric_core::ToolCall {
                    id: id.to_string(),
                    name: name.to_string(),
                    args,
                }],
                tool_call_id: None,
                media: Vec::new(),
            },
            input_tokens: Some(10),
            output_tokens: Some(5),
            truncated: false,
        }
    }

    /// test-critique C-005: `dispatch`'s unknown-*method* branch
    /// (`METHOD_NOT_FOUND`) has a dedicated error constant and code path but
    /// was never exercised — distinct from `tools_call_unknown_tool_is_json_rpc_error`
    /// (unknown *tool name* within a valid `tools/call`, `INVALID_PARAMS`).
    #[test]
    fn dispatch_unknown_method_is_json_rpc_error() {
        let dir = tempfile::tempdir().unwrap();
        let server = test_server(
            dir.path(),
            Box::new(crate::query::mock_provider(
                ferric_core::ActionProtocol::NativeTools,
            )),
        );
        let resp = server
            .dispatch(RpcRequest {
                id: Some(Value::from(1)),
                method: "nonsense/method".to_string(),
                params: Value::Null,
            })
            .unwrap();
        assert!(resp.result.is_none());
        assert_eq!(resp.error.unwrap().code, METHOD_NOT_FOUND);
    }

    #[test]
    fn tools_call_ferric_query_success() {
        let dir = tempfile::tempdir().unwrap();
        let server = test_server(
            dir.path(),
            Box::new(crate::query::mock_provider(
                ferric_core::ActionProtocol::NativeTools,
            )),
        );
        // build_run_config's caps for --mock select NativeTools (confirmed by
        // ferric-loop::protocol's own tests), so the mock script must match.
        assert_eq!(
            server.config.protocol,
            ferric_core::ActionProtocol::NativeTools
        );

        let resp = server
            .dispatch(call_request(
                1,
                serde_json::json!({"prompt": "do a mock task"}),
            ))
            .unwrap();
        let result = resp.result.unwrap();
        assert_eq!(result["isError"], false);
        assert_eq!(result["content"][0]["text"], "mock run complete");
        assert_eq!(result["structuredContent"]["status"], "completed");
        assert!(result["structuredContent"]["continuation_id"].is_null());
    }

    #[test]
    fn clarification_roundtrips_through_structured_mcp_continuation() {
        let dir = tempfile::tempdir().unwrap();
        let provider = ferric_provider::MockProvider::new(vec![
            native_completion(
                "ask-1",
                ferric_iron::terminator::REQUEST_USER_INPUT,
                serde_json::json!({
                    "question": "Which database?",
                    "context": "Both adapters exist and no default is documented.",
                    "options": ["SQLite", "PostgreSQL"]
                }),
            ),
            native_completion(
                "done-1",
                ferric_iron::terminator::TASK_COMPLETE,
                serde_json::json!({"summary": "used SQLite"}),
            ),
        ]);
        let server = test_server(dir.path(), Box::new(provider));

        let paused = server
            .dispatch(call_request(
                1,
                serde_json::json!({"prompt": "configure the database"}),
            ))
            .unwrap()
            .result
            .unwrap();
        assert_eq!(paused["isError"], false);
        assert_eq!(paused["structuredContent"]["status"], "needs_input");
        assert_eq!(
            paused["structuredContent"]["needs_input"]["request"]["question"],
            "Which database?"
        );
        let continuation = paused["structuredContent"]["continuation_id"]
            .as_str()
            .unwrap()
            .to_string();

        let invalid = server
            .dispatch(call_request(
                2,
                serde_json::json!({
                    "continuation_id": "mcp-wrong",
                    "answer": "SQLite"
                }),
            ))
            .unwrap();
        assert_eq!(invalid.error.unwrap().code, INVALID_PARAMS);
        assert!(
            server.resume.lock().unwrap().is_some(),
            "invalid input must not consume continuation state"
        );

        let completed = server
            .dispatch(call_request(
                3,
                serde_json::json!({
                    "continuation_id": continuation,
                    "answer": "SQLite"
                }),
            ))
            .unwrap()
            .result
            .unwrap();
        assert_eq!(completed["isError"], false);
        assert_eq!(completed["structuredContent"]["status"], "completed");
        assert_eq!(completed["content"][0]["text"], "used SQLite");
        assert!(server.resume.lock().unwrap().is_none());

        let traces = std::fs::read_dir(&server.trace_dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| std::fs::read_to_string(entry.path()).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(traces.contains("\"type\":\"resume_prompt\""));
        assert!(traces.contains("User answer: SQLite"));
    }

    #[test]
    fn launched_mock_server_resets_script_for_each_tools_call() {
        let dir = tempfile::tempdir().unwrap();
        let server = McpServer::launch(&base_mcp_args(dir.path())).unwrap();

        for (id, prompt) in [(1, "first mock task"), (2, "second mock task")] {
            let response = server
                .dispatch(call_request(id, serde_json::json!({"prompt": prompt})))
                .unwrap();
            let result = response.result.unwrap();
            assert_eq!(result["isError"], false, "request {id} failed: {result}");
            assert_eq!(result["content"][0]["text"], "mock run complete");
        }
    }

    #[test]
    fn tools_call_unknown_tool_is_json_rpc_error() {
        let dir = tempfile::tempdir().unwrap();
        let server = test_server(
            dir.path(),
            Box::new(crate::query::mock_provider(
                ferric_core::ActionProtocol::NativeTools,
            )),
        );
        let req = RpcRequest {
            id: Some(Value::from(1)),
            method: "tools/call".to_string(),
            params: serde_json::json!({"name": "not_a_real_tool", "arguments": {}}),
        };
        let resp = server.dispatch(req).unwrap();
        assert!(resp.result.is_none());
        assert_eq!(resp.error.unwrap().code, INVALID_PARAMS);
    }

    /// A provider that fails on the very first turn (`ProviderError::
    /// ScriptExhausted(0)`, an empty script) must surface as `isError:true`,
    /// not panic and not a raw JSON-RPC error.
    #[test]
    fn tools_call_provider_error_is_is_error_true_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let server = test_server(
            dir.path(),
            Box::new(ferric_provider::MockProvider::new(Vec::new())),
        );
        let resp = server
            .dispatch(call_request(1, serde_json::json!({"prompt": "do a task"})))
            .unwrap();
        let result = resp.result.unwrap();
        assert_eq!(result["isError"], true);
    }

    /// Reads the most recently written `mcp-*.jsonl` trace's `prompt_assembled`
    /// event — the same event `cli.rs`'s CLI-level file-routing tests inspect.
    fn latest_mcp_prompt_assembled(trace_dir: &std::path::Path) -> Value {
        let entry = std::fs::read_dir(trace_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("mcp-"))
            .max_by_key(|e| e.metadata().unwrap().modified().unwrap())
            .expect("a mcp-*.jsonl trace");
        let content = std::fs::read_to_string(entry.path()).unwrap();
        content
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .find(|v| v["event"]["type"] == "prompt_assembled")
            .expect("a prompt_assembled event")
    }

    /// `AppendText` branch: the file's content actually folds into the
    /// assembled prompt (checked via the trace's char count) — not merely
    /// "the call didn't error" (test-critique C-002: the prior version of
    /// this test asserted only `isError:false`, which can't distinguish
    /// "folded in" from "silently ignored").
    #[test]
    fn tools_call_file_text_folds_into_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes.md");
        let body = "MARKER ".repeat(50);
        std::fs::write(&notes, &body).unwrap();
        let server = test_server(
            dir.path(),
            Box::new(crate::query::mock_provider(
                ferric_core::ActionProtocol::NativeTools,
            )),
        );
        let resp = server
            .dispatch(call_request(
                1,
                serde_json::json!({"prompt": "summarize", "files": [notes.to_str().unwrap()]}),
            ))
            .unwrap();
        assert_eq!(resp.result.as_ref().unwrap()["isError"], false);
        let assembled = latest_mcp_prompt_assembled(&server.trace_dir);
        let chars = assembled["event"]["chars"].as_u64().unwrap();
        assert!(
            chars as usize >= body.len(),
            "assembled prompt ({chars} chars) should include the {}-char file",
            body.len()
        );
    }

    #[test]
    fn tools_call_file_outside_selected_workspace_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let outside = dir.path().join("outside.md");
        std::fs::write(&outside, "must not enter the prompt").unwrap();
        let server = test_server(
            &workspace,
            Box::new(crate::query::mock_provider(
                ferric_core::ActionProtocol::NativeTools,
            )),
        );

        let resp = server
            .dispatch(call_request(
                1,
                serde_json::json!({"prompt": "summarize", "files": [outside.to_str().unwrap()]}),
            ))
            .unwrap();
        let result = resp.result.unwrap();
        assert_eq!(result["isError"], true);
        let message = result["content"][0]["text"].as_str().unwrap();
        assert!(
            message.contains("workspace containment") && message.contains("escapes workspace"),
            "unexpected denial: {message}"
        );
    }

    struct CapturesMediaProvider {
        seen_media: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl Provider for CapturesMediaProvider {
        fn id(&self) -> &str {
            "captures-media"
        }

        fn capabilities(&self) -> ferric_provider::Capabilities {
            ferric_provider::Capabilities {
                supports_native_tool_calls: true,
                supports_constraint: false,
                supports_media: true,
            }
        }

        async fn complete(
            &self,
            request: ferric_provider::CompletionRequest,
            _cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
        ) -> Result<ferric_provider::Completion, ferric_provider::ProviderError> {
            use ferric_core::{Role, ToolCall};

            let count = request.messages.iter().map(|m| m.media.len()).sum();
            self.seen_media
                .store(count, std::sync::atomic::Ordering::SeqCst);
            Ok(ferric_provider::Completion {
                message: ferric_core::Message {
                    role: Role::Assistant,
                    text: None,
                    tool_calls: vec![ToolCall {
                        id: "media-0".to_string(),
                        name: ferric_iron::terminator::TASK_COMPLETE.to_string(),
                        args: serde_json::json!({"summary": "media received"}),
                    }],
                    tool_call_id: None,
                    media: Vec::new(),
                },
                input_tokens: Some(10),
                output_tokens: Some(5),
                truncated: false,
            })
        }
    }

    #[test]
    fn tools_call_valid_media_is_routed_to_the_provider() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("photo.png");
        std::fs::write(&photo, [0_u8, 1, 2]).unwrap();
        let seen_media = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut server = test_server(
            dir.path(),
            Box::new(CapturesMediaProvider {
                seen_media: std::sync::Arc::clone(&seen_media),
            }),
        );
        server.config.caps.supports_media = true;
        server.declared = vec![crate::media::Modality::Image];

        let result = server
            .dispatch(call_request(
                1,
                serde_json::json!({"prompt": "describe", "files": ["photo.png"]}),
            ))
            .unwrap()
            .result
            .unwrap();
        assert_eq!(result["isError"], false);
        assert_eq!(
            seen_media.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the provider must receive the routed image"
        );
    }

    /// A provider that fails its FIRST `complete()` call (non-retryable, so
    /// `run()` stops immediately with `ProviderError`) and succeeds every call
    /// after — proves a real recovery, not just "doesn't crash on repeat
    /// errors". One instance is shared across both `tools/call`s below, the
    /// same way one provider is shared for a real `ferric mcp` process's
    /// lifetime.
    struct FlakyOnceProvider {
        calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl Provider for FlakyOnceProvider {
        fn id(&self) -> &str {
            "flaky-once"
        }
        fn capabilities(&self) -> ferric_provider::Capabilities {
            ferric_provider::Capabilities {
                supports_native_tool_calls: true,
                supports_constraint: false,
                supports_media: false,
            }
        }
        async fn complete(
            &self,
            _request: ferric_provider::CompletionRequest,
            _cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
        ) -> Result<ferric_provider::Completion, ferric_provider::ProviderError> {
            use ferric_core::{Role, ToolCall};
            let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n == 0 {
                Err(ferric_provider::ProviderError::Backend(
                    "flaky: first call always fails".to_string(),
                ))
            } else {
                Ok(ferric_provider::Completion {
                    message: ferric_core::Message {
                        role: Role::Assistant,
                        text: None,
                        tool_calls: vec![ToolCall {
                            id: "flaky-0".to_string(),
                            name: ferric_iron::terminator::TASK_COMPLETE.to_string(),
                            args: serde_json::json!({"summary": "recovered"}),
                        }],
                        tool_call_id: None,
                        media: Vec::new(),
                    },
                    input_tokens: Some(10),
                    output_tokens: Some(5),
                    truncated: false,
                })
            }
        }
    }

    /// Two `tools/call`s through the SAME server/session: an error on the
    /// first must not corrupt or block the second (T-3605's "server SHALL
    /// continue... serving subsequent tools/call requests" clause) — and the
    /// second must actually SUCCEED, proving real recovery.
    #[test]
    fn error_then_success_same_session() {
        let dir = tempfile::tempdir().unwrap();
        let server = test_server(
            dir.path(),
            Box::new(FlakyOnceProvider {
                calls: std::sync::atomic::AtomicUsize::new(0),
            }),
        );

        let first = server
            .dispatch(call_request(1, serde_json::json!({"prompt": "task one"})))
            .unwrap();
        assert_eq!(first.result.as_ref().unwrap()["isError"], true);

        let second = server
            .dispatch(call_request(2, serde_json::json!({"prompt": "task two"})))
            .unwrap();
        assert_eq!(second.result.as_ref().unwrap()["isError"], false);
        assert_eq!(
            second.result.as_ref().unwrap()["content"][0]["text"],
            "recovered"
        );
        assert_eq!(second.id, Value::from(2));
    }

    /// The full MCP lifecycle through the dispatch function in-process:
    /// initialize → notifications/initialized → tools/list → tools/call.
    #[test]
    fn full_handshake_and_call_sequence() {
        let dir = tempfile::tempdir().unwrap();
        let server = test_server(
            dir.path(),
            Box::new(crate::query::mock_provider(
                ferric_core::ActionProtocol::NativeTools,
            )),
        );

        let init = server
            .dispatch(RpcRequest {
                id: Some(Value::from(1)),
                method: "initialize".to_string(),
                params: Value::Null,
            })
            .unwrap();
        assert!(init.result.is_some());

        let notified = server.dispatch(RpcRequest {
            id: None,
            method: "notifications/initialized".to_string(),
            params: Value::Null,
        });
        assert!(notified.is_none());

        let list = server
            .dispatch(RpcRequest {
                id: Some(Value::from(2)),
                method: "tools/list".to_string(),
                params: Value::Null,
            })
            .unwrap();
        assert_eq!(list.result.unwrap()["tools"][0]["name"], "ferric_query");

        let call = server
            .dispatch(call_request(
                3,
                serde_json::json!({"prompt": "do a mock task"}),
            ))
            .unwrap();
        assert_eq!(
            call.result.unwrap()["content"][0]["text"],
            "mock run complete"
        );
    }
}

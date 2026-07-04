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

use ferric_core::Modality;
use ferric_guard::Workspace;
use ferric_loop::StopReason;
use ferric_provider::Provider;
use ferric_trace::JsonlSink;

use crate::backend::BackendOpts;
use crate::query::{
    ProtocolArg, RunConfig, RunConfigArgs, build_run_config, mock_provider, now_ms, route_files,
    run_with_provider,
};

#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai"))]
use tokio::runtime::Runtime;

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
            "capabilities": {"tools": {}},
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
                    "description": "The task to perform.",
                },
                "files": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional paths to attach. Text/code folds into the \
                        prompt; images/audio/video attach as media if the server's \
                        declared --modality supports it.",
                },
            },
            "required": ["prompt"],
        },
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
    /// Workspace root (containment boundary). Default: current directory.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    #[command(flatten)]
    pub backend_opts: BackendOpts,

    /// Parameter count in billions. Default 1.2 when neither this flag nor a
    /// config file's `params_b` is set (T-3805).
    #[arg(long)]
    pub params_b: Option<f32>,

    /// Quantization label. Default "Q4_K_M" when neither this flag nor a
    /// config file's `quant` is set (T-3805).
    #[arg(long)]
    pub quant: Option<String>,

    /// Model family label. Default "unknown" when neither this flag nor a
    /// config file's `family` is set (T-3805).
    #[arg(long)]
    pub family: Option<String>,

    /// Context window in tokens (ModelProfile is config-supplied, ADR-006).
    /// Default 4096 when neither this flag nor a config file's `ctx` is set
    /// (T-3805).
    #[arg(long)]
    pub ctx: Option<u32>,

    /// Sampling temperature (0.0 selects the deterministic sampler). Default
    /// 0.0 when neither this flag nor a config file's `temperature` is set
    /// (T-3805).
    #[arg(long)]
    pub temperature: Option<f32>,

    /// Action protocol override (default: chosen from policy + backend caps)
    #[arg(long, value_enum)]
    pub protocol: Option<ProtocolArg>,

    /// Directory of prompt elements to compose the system prompt from.
    #[arg(long)]
    pub prompts_dir: Option<PathBuf>,

    /// Run against a built-in scripted mock instead of a real model
    #[arg(long)]
    pub mock: bool,

    /// Declare the model's accepted non-text modalities (comma list:
    /// `image,audio,video`). Applies to every `tools/call`'s `files`.
    #[arg(long)]
    pub modality: Option<String>,

    /// Cap the active tool ring (ADR-028).
    #[arg(long)]
    pub max_ring: Option<u8>,

    /// Directory holding `model_profiles.json` (ADR-029). Read once at
    /// launch — a later `ferric bench --calibrate-rings` run is picked up
    /// only on restart (ADR-046). Default "benchmarks" when neither this flag
    /// nor a config file's `profile_dir` is set (T-3805).
    #[arg(long)]
    pub profile_dir: Option<PathBuf>,
}

/// How `McpServer` drives a loop turn. `Mock` needs no ambient async runtime
/// (`futures_executor::block_on`, mirroring `drive_mock`); `Real` holds the ONE
/// `tokio::runtime::Runtime` built at launch and reused for every subsequent
/// `tools/call` (T-3601's reusable-provider shape, applied to the executor
/// too — see `run_with_provider`'s doc comment).
pub(crate) enum Executor {
    Mock,
    #[cfg(any(feature = "backend-mistralrs", feature = "backend-openai"))]
    Real(Runtime),
}

/// The long-lived state one `ferric mcp` process holds: workspace/backend/
/// model/protocol are fixed at launch (`RunConfig`, `Workspace`, the
/// provider) — nothing here is renegotiated per `tools/call`.
pub(crate) struct McpServer {
    pub workspace: Workspace,
    pub config: RunConfig,
    pub provider: Box<dyn Provider + Send + Sync>,
    pub executor: Executor,
    pub declared: Vec<Modality>,
    pub trace_dir: PathBuf,
}

fn tool_content(text: String, is_error: bool) -> Value {
    serde_json::json!({
        "content": [{"type": "text", "text": text}],
        "isError": is_error,
    })
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
            other => RpcResponse::error(id, METHOD_NOT_FOUND, format!("unknown method: {other}")),
        })
    }

    fn handle_tools_call(&self, id: Value, params: &Value) -> RpcResponse {
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        if name != "ferric_query" {
            return RpcResponse::error(id, INVALID_PARAMS, format!("unknown tool: {name}"));
        }
        let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
        let prompt = match arguments.get("prompt").and_then(Value::as_str) {
            Some(p) => p.to_string(),
            None => {
                return RpcResponse::error(
                    id,
                    INVALID_PARAMS,
                    "\"prompt\" is required and must be a string",
                );
            }
        };
        let files: Vec<PathBuf> = arguments
            .get("files")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default();

        let (media_parts, prompt_suffix) =
            route_files(&files, &self.declared, self.config.caps.supports_media);
        let effective_prompt = if prompt_suffix.is_empty() {
            prompt
        } else {
            format!("{prompt}{prompt_suffix}")
        };

        let session = format!("mcp-{}", now_ms());
        let trace_path = self.trace_dir.join(format!("{session}.jsonl"));
        let mut sink = match JsonlSink::open(&trace_path, &session) {
            Ok(sink) => sink,
            Err(e) => {
                return RpcResponse::success(
                    id,
                    tool_content(format!("cannot open trace: {e}"), true),
                );
            }
        };

        // A `ProviderError` stop is the only failure mode surfaced as
        // `isError:true` — the same convention `ferric query`'s CLI already
        // uses (`StopReason::ProviderError` ⇒ `ExitCode::FAILURE`, every other
        // stop reason ⇒ success). This keeps the two surfaces' semantics
        // aligned rather than inventing a second failure taxonomy.
        match self.run_one(&mut sink, &effective_prompt, media_parts) {
            Ok(outcome) if outcome.stop != StopReason::ProviderError => RpcResponse::success(
                id,
                tool_content(outcome.final_text.unwrap_or_default(), false),
            ),
            Ok(outcome) => RpcResponse::success(
                id,
                tool_content(
                    outcome
                        .final_text
                        .unwrap_or_else(|| "provider error".to_string()),
                    true,
                ),
            ),
            Err(message) => RpcResponse::success(id, tool_content(message, true)),
        }
    }

    fn run_one(
        &self,
        sink: &mut JsonlSink,
        prompt: &str,
        media: Vec<ferric_core::MediaPart>,
    ) -> Result<ferric_loop::LoopOutcome, String> {
        // MCP does not stream this sprint (ADR-046: stdout is reserved
        // exclusively for JSON-RPC frames; partial-text needs MCP's own
        // notification mechanism, a separate future increment).
        let fut = run_with_provider(
            self.provider.as_ref(),
            &self.config.registry,
            &self.workspace,
            &self.config.policy,
            self.config.protocol,
            self.config.sampling.clone(),
            self.config.system_prompt.as_deref(),
            self.config.lineage.clone(),
            sink,
            prompt,
            media,
            None,
        );
        match &self.executor {
            Executor::Mock => futures_executor::block_on(fut),
            #[cfg(any(feature = "backend-mistralrs", feature = "backend-openai"))]
            Executor::Real(rt) => rt.block_on(fut),
        }
    }

    /// Build the server: workspace + the launch-time-fixed run config + the
    /// provider, constructed exactly once (a real backend also gets exactly
    /// one `tokio::runtime::Runtime`, reused for every subsequent
    /// `tools/call` — see `run_one`).
    pub fn launch(args: &McpArgs) -> Result<Self, String> {
        let workspace_root = match &args.workspace {
            Some(path) => path.clone(),
            None => std::env::current_dir()
                .map_err(|e| format!("cannot determine current directory: {e}"))?,
        };
        let workspace = Workspace::new(&workspace_root).map_err(|e| format!("workspace: {e}"))?;

        // T-3805: layered config (CLI flag > project `.ferric/config.toml` >
        // user config > today's hardcoded default). `McpArgs` is `&`, not
        // owned, so the merge happens on a local clone of `backend_opts`
        // rather than in place (mirrors `run_query`'s T-3803 treatment) —
        // that clone is what both `RunConfigArgs` and `build_real_provider`
        // use below, so the config-resolved model/backend actually reaches
        // the real provider, not just protocol/tier selection.
        let loaded_config = crate::config::load_layered(&workspace_root);
        let cfg = loaded_config.config;
        let mut backend_opts = args.backend_opts.clone();
        backend_opts.backend = backend_opts.backend.take().or(cfg.backend);
        backend_opts.model_dir = backend_opts.model_dir.take().or(cfg.model_dir);
        backend_opts.model_file = backend_opts.model_file.take().or(cfg.model_file);
        backend_opts.model = backend_opts.model.take().or(cfg.model);
        backend_opts.api_base = backend_opts.api_base.take().or(cfg.api_base);
        backend_opts.api_key = backend_opts.api_key.take().or(cfg.api_key);
        let resolved_params_b = args.params_b.or(cfg.params_b).unwrap_or(1.2);
        let resolved_quant = args
            .quant
            .clone()
            .or(cfg.quant)
            .unwrap_or_else(|| "Q4_K_M".to_string());
        let resolved_family = args
            .family
            .clone()
            .or(cfg.family)
            .unwrap_or_else(|| "unknown".to_string());
        let resolved_ctx = args.ctx.or(cfg.ctx).unwrap_or(4096);
        let resolved_temperature = args.temperature.or(cfg.temperature).unwrap_or(0.0);
        let resolved_max_ring = args.max_ring.or(cfg.max_ring);
        let resolved_profile_dir = args
            .profile_dir
            .clone()
            .or(cfg.profile_dir)
            .unwrap_or_else(|| PathBuf::from("benchmarks"));

        let mut config = build_run_config(&RunConfigArgs {
            mock: args.mock,
            backend: backend_opts
                .backend
                .unwrap_or(crate::backend::BackendArg::Mistral),
            params_b: resolved_params_b,
            quant: resolved_quant,
            family: resolved_family,
            ctx: resolved_ctx,
            temperature: resolved_temperature,
            protocol_override: args.protocol,
            prompts_dir: args.prompts_dir.clone(),
            max_ring: resolved_max_ring,
            profile_dir: resolved_profile_dir,
            // C-001 (plan-critic): derived from the POST-merge, config-
            // resolved `model`/`model_file` (already merged above into
            // `backend_opts`).
            model_key: backend_opts
                .model
                .clone()
                .or_else(|| backend_opts.model_file.clone()),
        });
        // No trace sink exists yet at launch (each tools/call opens its own),
        // so a composition failure — and any malformed-config diagnostic
        // (C-004) — is surfaced once here rather than dropped, or (unlike
        // `Note`-tracing) repeated on every subsequent call.
        if let Some(err) = &config.prompt_composition_error {
            eprintln!("{err}");
        }
        for diag in &loaded_config.diagnostics {
            eprintln!("{diag}");
        }

        // T-3806: `Animus.md` — same fold as `run_query`. No sink exists yet
        // at launch (each `tools/call` opens its own), so its presence is
        // surfaced via `eprintln!` here rather than a per-call `Note` (same
        // treatment as the malformed-config diagnostics above).
        let animus_md = std::fs::read_to_string(workspace_root.join("Animus.md")).ok();
        if let Some(content) = &animus_md {
            config.system_prompt = Some(crate::query::fold_animus_md(
                config.system_prompt.as_deref(),
                content,
            ));
            eprintln!("Animus.md applied ({} chars)", content.len());
        }

        let trace_dir = workspace_root.join(".ferric").join("trace");
        std::fs::create_dir_all(&trace_dir)
            .map_err(|e| format!("cannot create trace dir {}: {e}", trace_dir.display()))?;
        let declared = ferric_core::parse_modalities(args.modality.as_deref().unwrap_or(""));

        let (provider, executor): (Box<dyn Provider + Send + Sync>, Executor) = if args.mock {
            (Box::new(mock_provider(config.protocol)), Executor::Mock)
        } else {
            build_real_provider(&backend_opts)?
        };

        Ok(McpServer {
            workspace,
            config,
            provider,
            executor,
            declared,
            trace_dir,
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

#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai"))]
fn build_real_provider(
    backend_opts: &BackendOpts,
) -> Result<(Box<dyn Provider + Send + Sync>, Executor), String> {
    let runtime = Runtime::new().map_err(|e| format!("tokio runtime: {e}"))?;
    let provider = runtime.block_on(crate::backend::create_provider(backend_opts))?;
    Ok((provider, Executor::Real(runtime)))
}

#[cfg(not(any(feature = "backend-mistralrs", feature = "backend-openai")))]
fn build_real_provider(
    _backend_opts: &BackendOpts,
) -> Result<(Box<dyn Provider + Send + Sync>, Executor), String> {
    Err("this binary was built without backend features; \
         rebuild with `cargo build --features backend-mistralrs,backend-openai`, or use --mock"
        .to_string())
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

    /// T-3804: same regression as `cli::query_defaults_unchanged_after_clap_
    /// type_change`, scoped to `ferric mcp`. `McpArgs` built with every
    /// formerly-clap-defaulted field `None` and no config file involved yet
    /// (T-3805) — `McpServer::launch`'s resolved tier must land at
    /// `Tier::Nano` (default `--params-b` 1.2), matching pre-refactor
    /// behavior exactly.
    #[test]
    fn launch_defaults_unchanged_after_clap_type_change() {
        let dir = tempfile::tempdir().unwrap();
        let args = base_mcp_args(dir.path());
        let server = McpServer::launch(&args).unwrap();
        assert_eq!(server.config.policy.tier, ferric_core::Tier::Nano);
        assert_eq!(server.config.policy.max_output_tokens, 512);
    }

    /// All-`None`/`mock: true` `McpArgs` for a given workspace — the shared
    /// base the T-3805 config-precedence tests below start from.
    fn base_mcp_args(ws: &std::path::Path) -> McpArgs {
        McpArgs {
            workspace: Some(ws.to_path_buf()),
            backend_opts: crate::backend::BackendOpts {
                backend: None,
                model_dir: None,
                model_file: None,
                model: None,
                api_base: None,
                api_key: None,
                chat_template: None,
                tokenizer_json: None,
                tok_model_id: None,
            },
            params_b: None,
            quant: None,
            family: None,
            ctx: None,
            temperature: None,
            protocol: None,
            prompts_dir: None,
            mock: true,
            modality: None,
            max_ring: None,
            profile_dir: None,
        }
    }

    fn write_project_config(ws: &std::path::Path, toml: &str) {
        std::fs::create_dir_all(ws.join(".ferric")).unwrap();
        std::fs::write(ws.join(".ferric").join("config.toml"), toml).unwrap();
    }

    /// T-3805: same shape as `cli::config_file_sets_default_without_flag`.
    #[test]
    fn launch_config_file_sets_default_without_flag() {
        let dir = tempfile::tempdir().unwrap();
        write_project_config(dir.path(), "params_b = 8.0\n");
        let server = McpServer::launch(&base_mcp_args(dir.path())).unwrap();
        assert_eq!(server.config.policy.tier, ferric_core::Tier::Small);
    }

    /// T-3805: same shape as `cli::cli_flag_overrides_config_file`.
    #[test]
    fn launch_cli_flag_overrides_config_file() {
        let dir = tempfile::tempdir().unwrap();
        write_project_config(dir.path(), "params_b = 8.0\n");
        let mut args = base_mcp_args(dir.path());
        args.params_b = Some(1.2);
        let server = McpServer::launch(&args).unwrap();
        assert_eq!(server.config.policy.tier, ferric_core::Tier::Nano);
    }

    /// C-001 (plan-critic): same shape as `cli::config_only_model_still_
    /// resolves_profile`, scoped to `ferric mcp` — `model` set ONLY via
    /// config.toml must still hit the persisted `calibrated_ring` lookup.
    #[test]
    fn launch_config_only_model_still_resolves_profile() {
        let pdir = tempfile::tempdir().unwrap();
        std::fs::write(
            pdir.path().join("model_profiles.json"),
            r#"[{"model":"mockmodel","params_b":8.0,"protocol":"ConstrainedJson","measured_level":null,"tier_from_params":"Small","tier_from_measured":null,"calibrated_ring":0}]"#,
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        write_project_config(
            dir.path(),
            &format!(
                "model = \"mockmodel\"\nprofile_dir = '{}'\n",
                pdir.path().display()
            ),
        );
        let mut args = base_mcp_args(dir.path());
        args.params_b = Some(8.0);
        args.protocol = Some(ProtocolArg::Grammar);
        let server = McpServer::launch(&args).unwrap();
        assert_eq!(server.config.policy.max_ring, Some(0));
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
    }

    fn test_server(dir: &std::path::Path, provider: Box<dyn Provider + Send + Sync>) -> McpServer {
        let workspace = Workspace::new(dir).unwrap();
        let config = crate::query::build_run_config(&crate::query::RunConfigArgs {
            mock: true,
            backend: crate::backend::BackendArg::Mistral,
            params_b: 1.2,
            quant: "Q4_K_M".to_string(),
            family: "unknown".to_string(),
            ctx: 4096,
            temperature: 0.0,
            protocol_override: None,
            prompts_dir: None,
            max_ring: None,
            profile_dir: PathBuf::from("benchmarks"),
            model_key: None,
        });
        let trace_dir = dir.join(".ferric").join("trace");
        std::fs::create_dir_all(&trace_dir).unwrap();
        McpServer {
            workspace,
            config,
            provider,
            executor: Executor::Mock,
            declared: Vec::new(),
            trace_dir,
        }
    }

    fn call_request(id: i64, arguments: Value) -> RpcRequest {
        RpcRequest {
            id: Some(Value::from(id)),
            method: "tools/call".to_string(),
            params: serde_json::json!({"name": "ferric_query", "arguments": arguments}),
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

    /// `Skip` branch (test-critique C-001: the locked test plan required all
    /// three `Attachment` branches; only `AppendText` shipped). A media file
    /// with no declared modality is dropped non-fatally — the
    /// security-relevant path (an undeclared attachment must never silently
    /// attach). Mirrors `cli.rs`'s `query_file_media_skipped_with_reason`.
    /// (The `Media`-attaches case is not separately testable here: `--mock`
    /// hardcodes `caps.supports_media = false`, so `decide_attachment` always
    /// routes media to `Skip` regardless of `declared` — the same is true of
    /// the existing CLI test suite, which likewise has no "media attaches"
    /// case; this is parity with, not a regression from, prior coverage.)
    #[test]
    fn tools_call_file_media_skipped_with_reason() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("photo.png");
        std::fs::write(&photo, [0u8; 16]).unwrap();
        let server = test_server(
            dir.path(),
            Box::new(crate::query::mock_provider(
                ferric_core::ActionProtocol::NativeTools,
            )),
        );
        let resp = server
            .dispatch(call_request(
                1,
                serde_json::json!({"prompt": "describe", "files": [photo.to_str().unwrap()]}),
            ))
            .unwrap();
        // Skip is non-fatal: the call still succeeds, just without the media.
        assert_eq!(resp.result.unwrap()["isError"], false);
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
                exposes_logits: false,
                supports_media: false,
            }
        }
        async fn complete(
            &self,
            _request: ferric_provider::CompletionRequest,
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
                            name: ferric_loop::TASK_COMPLETE.to_string(),
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

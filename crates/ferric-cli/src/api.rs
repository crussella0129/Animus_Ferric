//! `ferric api` — HTTP API server for Animus Ferric (Sprint 64).
//!
//! Exposes the existing agentic loop over REST + SSE streaming, so any HTTP
//! client (Animus IDE, web UI, mobile app) can drive the agent. Same
//! constrained loop, same guard, same trace — just an HTTP surface.
//!
//! Binds to `127.0.0.1:3581` by default (ADR-005 parity). Override with
//! `--host` and `--port`, or `--tunnel` to listen on the Ferric WireGuard
//! tunnel's host address instead.
//!
//! Every request passes the network gate (sprint 127, INT-0015): this
//! listener's `Host` (no DNS rebinding), no browser `Origin`/`Sec-Fetch-*`,
//! the API's bearer token, and JSON bodies. `GET /health` skips only the
//! token.

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::sse::{Event, Sse},
    routing::{get, post},
};
use clap::Args;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use ferric_guard::Workspace;
use ferric_loop::RunArgs;
use ferric_provider::StreamDelta;

use crate::backend::BackendOpts;
use crate::config::RunFlags;
use crate::query::{
    RunConfig, RunConfigError, build_run_config, ensure_supported_harness_policy, route_files,
    run_with_provider,
};

/// CLI arguments for `ferric api`.
#[derive(Args)]
pub struct ApiArgs {
    /// Host to bind to. Default: 127.0.0.1 (loopback only, ADR-005 parity).
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Port to bind to. Default: 3581.
    #[arg(long, default_value_t = 3581)]
    pub port: u16,

    #[command(flatten)]
    pub run: RunFlags,

    /// Declare the model's accepted non-text modalities (comma list:
    /// `image,audio,video`). Applies to every request's `files`.
    #[arg(long)]
    pub modality: Option<String>,

    /// The API's bearer-token file (created mode 0600 when missing).
    /// Default: `<state dir>/ferric/gate/api.token`.
    #[arg(long)]
    pub token_file: Option<PathBuf>,

    /// Listen on the Ferric tunnel's host address (from `ferric tunnel
    /// init`) instead of `--host`. The bearer token is still required.
    #[arg(long)]
    pub tunnel: bool,

    /// Tunnel plan directory. Default: `<state dir>/ferric/tunnel`.
    #[arg(long)]
    pub tunnel_dir: Option<PathBuf>,
}

/// Shared application state passed to every handler.
struct AppState {
    workspace: Workspace,
    args: ApiArgs,
}

/// The request body for query and chat endpoints.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryRequest {
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub files: Vec<String>,
    /// Opaque prior API session id; never a caller-supplied path.
    #[serde(default)]
    pub continuation_id: Option<String>,
    /// Explicit answer to a pending clarification request.
    #[serde(default)]
    pub answer: Option<String>,
}

/// The JSON response for non-streaming query.
#[derive(Debug, Serialize)]
pub struct QueryResponse {
    pub text: Option<String>,
    pub turns: u32,
    pub stop_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub needs_input: Option<ferric_loop::NeedsInput>,
}

#[derive(Debug)]
enum ApiQueryError {
    InvalidRequest(String),
    InvalidAttachment(String),
    InvalidContinuation(String),
    Internal(String),
}

impl From<RunConfigError> for ApiQueryError {
    fn from(error: RunConfigError) -> Self {
        match error {
            RunConfigError::UnsupportedPolicy(message) => Self::InvalidRequest(message.to_string()),
            RunConfigError::Config(error) => Self::Internal(error.to_string()),
        }
    }
}

impl ApiQueryError {
    fn into_http(self) -> (StatusCode, String) {
        match self {
            Self::InvalidRequest(message)
            | Self::InvalidAttachment(message)
            | Self::InvalidContinuation(message) => (StatusCode::BAD_REQUEST, message),
            Self::Internal(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
        }
    }

    fn message(self) -> String {
        match self {
            Self::InvalidRequest(message)
            | Self::InvalidAttachment(message)
            | Self::InvalidContinuation(message)
            | Self::Internal(message) => message,
        }
    }
}

#[derive(Debug)]
enum RequestMode<'a> {
    New {
        prompt: &'a str,
    },
    Resume {
        continuation_id: &'a str,
        answer: &'a str,
    },
}

fn request_mode(request: &QueryRequest) -> Result<RequestMode<'_>, ApiQueryError> {
    match (
        request.prompt.as_deref(),
        request.continuation_id.as_deref(),
        request.answer.as_deref(),
    ) {
        (Some(prompt), None, None) if !prompt.trim().is_empty() => Ok(RequestMode::New { prompt }),
        (None, Some(continuation_id), Some(answer)) if !answer.trim().is_empty() => {
            if !request.files.is_empty() {
                return Err(ApiQueryError::InvalidRequest(
                    "files are not accepted when answering a continuation".to_string(),
                ));
            }
            Ok(RequestMode::Resume {
                continuation_id,
                answer,
            })
        }
        _ => Err(ApiQueryError::InvalidRequest(
            "supply either a non-empty prompt, or continuation_id plus a non-empty answer"
                .to_string(),
        )),
    }
}

fn continuation_path(trace_dir: &std::path::Path, id: &str) -> Result<PathBuf, ApiQueryError> {
    if id.is_empty()
        || id.len() > 160
        || !id.starts_with("api-")
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ApiQueryError::InvalidContinuation(
            "invalid continuation_id".to_string(),
        ));
    }
    let path = trace_dir.join(format!("{id}.jsonl"));
    if !path.is_file() {
        return Err(ApiQueryError::InvalidContinuation(format!(
            "unknown continuation_id {id:?}"
        )));
    }
    Ok(path)
}

/// Health check handler.
async fn health() -> Json<serde_json::Value> {
    Json(
        serde_json::json!({"status": "ok", "service": "ferric-api", "version": env!("CARGO_PKG_VERSION")}),
    )
}

/// Non-streaming query handler.
async fn query_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<QueryResponse>, (StatusCode, String)> {
    let outcome = run_query(&state, &req, None)
        .await
        .map_err(ApiQueryError::into_http)?;
    Ok(Json(outcome))
}

/// Streaming query handler — returns SSE events.
async fn query_stream_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<QueryRequest>,
) -> Sse<ReceiverStream<Result<Event, std::convert::Infallible>>> {
    let (tx, rx) = mpsc::channel::<Result<Event, std::convert::Infallible>>(1024);
    let state = Arc::clone(&state);
    let request = req.clone();

    tokio::spawn(async move {
        let tx_clone = tx.clone();
        let sink_fn = move |d: StreamDelta| {
            let event = match d {
                StreamDelta::Thought(t) => Event::default()
                    .event("thought")
                    .data(serde_json::json!({"text": t}).to_string()),
                StreamDelta::ToolNamed(name) => Event::default()
                    .event("tool")
                    .data(serde_json::json!({"name": name}).to_string()),
                StreamDelta::Text(t) => Event::default()
                    .event("summary")
                    .data(serde_json::json!({"text": t}).to_string()),
                StreamDelta::ToolCompleted { name, summary } => Event::default()
                    .event("tool_completed")
                    .data(serde_json::json!({"name": name, "summary": summary}).to_string()),
            };
            let _ = tx_clone.try_send(Ok(event));
        };

        match run_query(&state, &request, Some(&sink_fn)).await {
            Ok(outcome) => {
                let done_event = Event::default().event("done").data(
                    serde_json::json!({
                        "turns": outcome.turns,
                        "stop_reason": outcome.stop_reason,
                        "text": outcome.text,
                        "needs_input": outcome.needs_input,
                    })
                    .to_string(),
                );
                let _ = tx.send(Ok(done_event)).await;
            }
            Err(e) => {
                let err_event = Event::default()
                    .event("error")
                    .data(serde_json::json!({"error": e.message()}).to_string());
                let _ = tx.send(Ok(err_event)).await;
            }
        }
    });

    Sse::new(ReceiverStream::new(rx))
}

/// Core query executor, shared by streaming and non-streaming paths.
async fn run_query(
    state: &AppState,
    request: &QueryRequest,
    sink_fn: Option<&(dyn Fn(StreamDelta) + Sync)>,
) -> Result<QueryResponse, ApiQueryError> {
    let mode = request_mode(request)?;
    let args = &state.args;
    let (backend_opts, config) = api_run_config(&state.workspace, args)?;

    let trace_dir = ferric_trace::trace_dir(state.workspace.root());
    let (effective_prompt, media, resume, answer) = match mode {
        RequestMode::New { prompt } => {
            let file_paths: Vec<PathBuf> = request.files.iter().map(PathBuf::from).collect();
            let declared = crate::media::parse_modalities(args.modality.as_deref().unwrap_or(""));
            let (media, prompt_suffix) = route_files(
                &state.workspace,
                &file_paths,
                &declared,
                config.caps.supports_media,
            )
            .map_err(|e| ApiQueryError::InvalidAttachment(format!("files: {e}")))?;
            let prompt = crate::query::with_attachments(Some(prompt.to_string()), prompt_suffix);
            (prompt, media, None, None)
        }
        RequestMode::Resume {
            continuation_id,
            answer,
        } => {
            let path = continuation_path(&trace_dir, continuation_id)?;
            let replayed = ferric_loop::replay(&path).map_err(|error| {
                ApiQueryError::InvalidContinuation(format!(
                    "cannot resume continuation {continuation_id:?}: {error}"
                ))
            })?;
            ferric_loop::validate_resume_target(
                &replayed,
                state.workspace.root(),
                config.protocol,
                config.harness_policy,
            )
            .map_err(|error| ApiQueryError::InvalidContinuation(error.to_string()))?;
            (None, Vec::new(), Some(replayed), Some(answer))
        }
    };

    let (_session, _trace_path, mut sink) = crate::query::create_trace_sink(&trace_dir, "api")
        .map_err(|e| ApiQueryError::Internal(format!("trace open: {e}")))?;

    let provider: Box<dyn ferric_provider::Provider> = if args.run.mock {
        Box::new(crate::query::mock_provider(config.protocol))
    } else {
        crate::backend::create_provider_in(&backend_opts, state.workspace.root())
            .await
            .map_err(ApiQueryError::Internal)?
    };

    let outcome = run_with_provider(
        RunArgs {
            media,
            stream_sink: sink_fn,
            resume,
            answer,
            ..config.run_args(&state.workspace, provider.as_ref())
        },
        &mut sink,
        effective_prompt.as_deref(),
    )
    .await
    .map_err(|e| ApiQueryError::Internal(format!("query failed: {e}")))?;

    Ok(QueryResponse {
        text: outcome.final_text.clone(),
        turns: outcome.turns,
        stop_reason: outcome.stop.as_str().to_string(),
        needs_input: outcome.needs_input.clone(),
    })
}

/// Resolve one API request's effective run configuration without allocating
/// a trace, constructing a provider, binding a socket, or entering the
/// server future. Keeping this seam shared with `run_query` makes
/// surface-policy propagation directly testable and bounded.
fn api_run_config(
    workspace: &Workspace,
    args: &ApiArgs,
) -> Result<(BackendOpts, RunConfig), RunConfigError> {
    let cfg = crate::config::load_layered(workspace.root())?.config;
    // The HTTP caller is not necessarily the workspace owner, so a
    // workspace-level allowlist is not evidence that *this* requester
    // authorized anything. Skills stay off here until the API has a notion
    // of who is asking (ADR-091).
    let run = args.run.resolve(&cfg, workspace.root());
    let config = build_run_config(&run)?;
    Ok((run.backend, config))
}

/// Entry point for `ferric api`.
pub async fn run_api(args: ApiArgs) -> std::process::ExitCode {
    crate::exit_with(serve(args).await)
}

async fn serve(args: ApiArgs) -> Result<std::process::ExitCode, String> {
    let tunnel_ip = if args.tunnel {
        let dir = args
            .tunnel_dir
            .clone()
            .or_else(ferric_gate::tunnel::default_tunnel_dir)
            .ok_or("api: no state directory; pass --tunnel-dir")?;
        let info = ferric_gate::tunnel::TunnelInfo::load(&dir)
            .map_err(|error| format!("api: {error} (run `ferric tunnel init` first)"))?;
        Some(info.host_address)
    } else {
        None
    };
    let bind_host = tunnel_ip.map_or_else(|| args.host.clone(), |ip| ip.to_string());
    if !is_loopback_host(&bind_host) {
        bind_host
            .parse::<std::net::IpAddr>()
            .map_err(|_| {
                format!(
                    "refusing API bind to '{bind_host}'; use 127.0.0.1, localhost, ::1, or --tunnel"
                )
            })
            .and_then(|ip| ferric_gate::check_bind(ip, tunnel_ip))
            .map_err(|message| format!("api: {message}"))?;
    }

    let workspace_root = args
        .run
        .workspace
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let workspace = Workspace::new(&workspace_root).map_err(|e| format!("workspace error: {e}"))?;

    // Validate launch inputs before binding. The existing per-request
    // configuration reload remains in api_run_config.
    let launch_config = crate::config::load_layered(&workspace_root)
        .map_err(|error| format!("api: {error}"))?
        .config;
    let launch = args.run.resolve(&launch_config, &workspace_root);
    crate::config::validate_effective_numbers(
        launch.params_b,
        launch.ctx,
        launch.temperature,
        launch.max_ring,
    )
    .map_err(|error| format!("api: {error}"))?;
    ensure_supported_harness_policy(launch.harness_policy)
        .map_err(|error| format!("api: {error}"))?;

    let token_path = args
        .token_file
        .clone()
        .or_else(|| ferric_gate::default_token_path("api"))
        .ok_or("api: no state directory; pass --token-file")?;
    let (token, token_created) =
        ferric_gate::Token::load_or_create(&token_path).map_err(|error| format!("api: {error}"))?;

    let bind_addr = if bind_host.contains(':') {
        format!("[{}]:{}", bind_host, args.port)
    } else {
        format!("{}:{}", bind_host, args.port)
    };

    let state = Arc::new(AppState { workspace, args });

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .map_err(|e| format!("bind failed: {e}"))?;
    let local = listener
        .local_addr()
        .map_err(|e| format!("bind failed: {e}"))?;
    // The gate admits this listener's own authority plus the loopback
    // names for its port, and nothing else.
    let gate = ferric_gate::Gate::new(&token, local.port())
        .allow_host(&local.to_string())
        .open_path("/health");

    let app = ferric_gate::protect(
        Router::new()
            .route("/health", get(health))
            .route("/v1/query", post(query_handler))
            .route("/v1/query/stream", post(query_stream_handler))
            .with_state(state),
        gate,
    );

    eprintln!("Ferric API listening on http://{local}");
    eprintln!("  GET  /health          — health check (no token)");
    eprintln!("  POST /v1/query        — run agentic query (JSON response)");
    eprintln!("  POST /v1/query/stream  — run agentic query (SSE stream)");
    eprintln!(
        "  bearer token {} {}",
        if token_created {
            "minted at"
        } else {
            "read from"
        },
        token_path.display()
    );

    axum::serve(listener, app)
        .await
        .map_err(|e| format!("server error: {e}"))?;
    Ok(std::process::ExitCode::SUCCESS)
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|addr| addr.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HarnessPolicyArg;

    fn api_args(workspace: &std::path::Path) -> ApiArgs {
        ApiArgs {
            host: "127.0.0.1".to_string(),
            port: 0,
            run: RunFlags {
                workspace: Some(workspace.to_path_buf()),
                mock: true,
                ..RunFlags::default()
            },
            modality: None,
            token_file: Some(workspace.join("api-test.token")),
            tunnel: false,
            tunnel_dir: None,
        }
    }

    #[test]
    fn api_bind_is_restricted_to_loopback() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("127.42.0.9"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("localhost"));
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("192.0.2.10"));
        assert!(!is_loopback_host("example-host"));
    }

    #[test]
    fn unsupported_planner_fails_before_api_bind_or_trace() {
        let runtime = tokio::runtime::Runtime::new().unwrap();

        let cli_workspace = tempfile::tempdir().unwrap();
        let mut explicit = api_args(cli_workspace.path());
        explicit.run.harness_policy = Some(HarnessPolicyArg::EvidencePlanner);
        assert_eq!(
            runtime.block_on(run_api(explicit)),
            std::process::ExitCode::FAILURE
        );
        assert!(!cli_workspace.path().join(".ferric/trace").exists());

        let config_workspace = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(config_workspace.path().join(".ferric")).unwrap();
        std::fs::write(
            config_workspace.path().join(".ferric/config.toml"),
            "harness_policy = \"evidence_planner\"\n",
        )
        .unwrap();
        assert_eq!(
            runtime.block_on(run_api(api_args(config_workspace.path()))),
            std::process::ExitCode::FAILURE
        );
        assert!(!config_workspace.path().join(".ferric/trace").exists());
    }

    #[test]
    fn present_invalid_config_blocks_all_consumers_api_reload() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState {
            workspace: Workspace::new(dir.path()).unwrap(),
            args: api_args(dir.path()),
        };
        std::fs::create_dir(dir.path().join(".ferric")).unwrap();
        std::fs::write(
            dir.path().join(".ferric/config.toml"),
            "api_key = 'private-fixture-token' malformed",
        )
        .unwrap();
        let request = QueryRequest {
            prompt: Some("task".to_string()),
            files: Vec::new(),
            continuation_id: None,
            answer: None,
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime
            .block_on(run_query(&state, &request, None))
            .unwrap_err();
        let (status, message) = error.into_http();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(message.contains("configuration"));
        assert!(!message.contains("private-fixture-token"));
        assert!(!dir.path().join(".ferric/trace").exists());
    }

    #[test]
    fn legacy_prompt_request_keeps_its_wire_shape() {
        let request: QueryRequest = serde_json::from_value(serde_json::json!({
            "prompt": "do the task",
            "files": ["notes.md"]
        }))
        .unwrap();
        assert!(matches!(
            request_mode(&request),
            Ok(RequestMode::New {
                prompt: "do the task"
            })
        ));

        let response = QueryResponse {
            text: Some("done".to_string()),
            turns: 2,
            stop_reason: "task_complete".to_string(),
            needs_input: None,
        };
        let value = serde_json::to_value(response).unwrap();
        assert!(value.get("needs_input").is_none());
    }

    #[test]
    fn continuation_request_requires_one_nonblank_answer_mode() {
        let valid: QueryRequest = serde_json::from_value(serde_json::json!({
            "continuation_id": "api-123-4-5",
            "answer": "SQLite"
        }))
        .unwrap();
        assert!(matches!(
            request_mode(&valid),
            Ok(RequestMode::Resume {
                continuation_id: "api-123-4-5",
                answer: "SQLite"
            })
        ));

        for invalid in [
            serde_json::json!({}),
            serde_json::json!({"prompt": "task", "answer": "SQLite"}),
            serde_json::json!({"continuation_id": "api-1", "answer": " "}),
            serde_json::json!({
                "continuation_id": "api-1",
                "answer": "SQLite",
                "files": ["notes.md"]
            }),
        ] {
            let request: QueryRequest = serde_json::from_value(invalid).unwrap();
            assert!(request_mode(&request).is_err());
        }
    }

    #[test]
    fn continuation_id_is_an_opaque_workspace_scoped_name() {
        let dir = tempfile::tempdir().unwrap();
        let valid = "api-123-4-5";
        let expected = dir.path().join(format!("{valid}.jsonl"));
        std::fs::write(&expected, "").unwrap();
        assert_eq!(continuation_path(dir.path(), valid).unwrap(), expected);

        for invalid in ["../outside", "api/child", "q-123", "api-💥", ""] {
            assert!(continuation_path(dir.path(), invalid).is_err(), "{invalid}");
        }
    }
}

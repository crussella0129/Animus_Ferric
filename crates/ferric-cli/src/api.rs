//! `ferric api` — HTTP API server for Animus Ferric (Sprint 64).
//!
//! Exposes the existing agentic loop over REST + SSE streaming, so any HTTP
//! client (Animus IDE, web UI, mobile app) can drive the agent. Same
//! constrained loop, same guard, same trace — just an HTTP surface.
//!
//! Binds to `127.0.0.1:3581` by default (ADR-005 parity). Override with
//! `--host` and `--port`.

#[cfg(feature = "backend-openai")]
pub mod server {
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
    use ferric_loop::StopReason;
    use ferric_provider::StreamDelta;
    use ferric_trace::JsonlSink;

    use crate::backend::BackendOpts;
    use crate::query::{
        ProtocolArg, RunConfigArgs, build_run_config, now_ms,
        run_with_provider,
    };

    /// CLI arguments for `ferric api`.
    #[derive(Args)]
    pub struct ApiArgs {
        /// Workspace root (containment boundary). Default: current directory.
        #[arg(long)]
        pub workspace: Option<PathBuf>,

        /// Host to bind to. Default: 127.0.0.1 (loopback only, ADR-005 parity).
        #[arg(long, default_value = "127.0.0.1")]
        pub host: String,

        /// Port to bind to. Default: 3581.
        #[arg(long, default_value_t = 3581)]
        pub port: u16,

        #[command(flatten)]
        pub backend_opts: BackendOpts,

        /// Parameter count in billions. Default 1.2 when unset.
        #[arg(long)]
        pub params_b: Option<f32>,

        /// Quantization label. Default "Q4_K_M" when unset.
        #[arg(long)]
        pub quant: Option<String>,

        /// Model family label. Default "unknown" when unset.
        #[arg(long)]
        pub family: Option<String>,

        /// Context window in tokens.
        #[arg(long)]
        pub ctx: Option<u32>,

        /// Sampling temperature. Default 0.0.
        #[arg(long)]
        pub temperature: Option<f32>,

        /// Action protocol override.
        #[arg(long, value_enum)]
        pub protocol: Option<ProtocolArg>,

        /// Directory of prompt elements.
        #[arg(long)]
        pub prompts_dir: Option<PathBuf>,

        /// Cap the active tool ring.
        #[arg(long)]
        pub max_ring: Option<u8>,

        /// Directory holding `model_profiles.json`.
        #[arg(long)]
        pub profile_dir: Option<PathBuf>,

        /// Run against a built-in scripted mock.
        #[arg(long)]
        pub mock: bool,
    }

    /// Shared application state passed to every handler.
    struct AppState {
        workspace: Workspace,
        args: ApiArgs,
    }

    /// The request body for query and chat endpoints.
    #[derive(Debug, Deserialize)]
    pub struct QueryRequest {
        pub prompt: String,
        #[serde(default)]
        pub workspace: Option<String>,
        #[serde(default)]
        pub files: Vec<String>,
    }

    /// The JSON response for non-streaming query.
    #[derive(Debug, Serialize)]
    pub struct QueryResponse {
        pub text: Option<String>,
        pub turns: u32,
        pub stop_reason: String,
    }

    /// SSE event wrapper for streaming.
    #[derive(Debug, Serialize)]
    pub struct SsePayload {
        pub text: Option<String>,
        pub name: Option<String>,
    }

    /// Health check handler.
    async fn health() -> Json<serde_json::Value> {
        Json(serde_json::json!({"status": "ok", "service": "ferric-api", "version": env!("CARGO_PKG_VERSION")}))
    }

    /// Non-streaming query handler.
    async fn query_handler(
        State(state): State<Arc<AppState>>,
        Json(req): Json<QueryRequest>,
    ) -> Result<Json<QueryResponse>, (StatusCode, String)> {
        let outcome = run_query(&state, &req.prompt, false, None)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
        Ok(Json(outcome))
    }

    /// Streaming query handler — returns SSE events.
    async fn query_stream_handler(
        State(state): State<Arc<AppState>>,
        Json(req): Json<QueryRequest>,
    ) -> Sse<ReceiverStream<Result<Event, std::convert::Infallible>>> {
        let (tx, rx) = mpsc::channel::<Result<Event, std::convert::Infallible>>(1024);
        let state = Arc::clone(&state);
        let prompt = req.prompt.clone();

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
                };
                let _ = tx_clone.try_send(Ok(event));
            };

            match run_query(&state, &prompt, true, Some(&sink_fn)).await {
                Ok(outcome) => {
                    let done_event = Event::default()
                        .event("done")
                        .data(serde_json::json!({
                            "turns": outcome.turns,
                            "stop_reason": outcome.stop_reason,
                            "text": outcome.text,
                        }).to_string());
                    let _ = tx.send(Ok(done_event)).await;
                }
                Err(e) => {
                    let err_event = Event::default()
                        .event("error")
                        .data(serde_json::json!({"error": e}).to_string());
                    let _ = tx.send(Ok(err_event)).await;
                }
            }
        });

        Sse::new(ReceiverStream::new(rx))
    }

    /// Core query executor, shared by streaming and non-streaming paths.
    async fn run_query(
        state: &AppState,
        prompt: &str,
        stream: bool,
        sink_fn: Option<&(dyn Fn(StreamDelta) + Sync)>,
    ) -> Result<QueryResponse, String> {
        let args = &state.args;
        let loaded_config = crate::config::load_layered(state.workspace.root());
        let cfg = loaded_config.config;
        let backend_opts = crate::config::merge_backend_opts(args.backend_opts.clone(), &cfg);

        let config = build_run_config(&RunConfigArgs {
            mock: args.mock,
            backend: backend_opts
                .backend
                .unwrap_or(crate::backend::BackendArg::Openai),
            params_b: args.params_b.or(cfg.params_b).unwrap_or(1.2),
            quant: args
                .quant
                .clone()
                .or(cfg.quant)
                .unwrap_or_else(|| "Q4_K_M".to_string()),
            family: args
                .family
                .clone()
                .or(cfg.family)
                .unwrap_or_else(|| "unknown".to_string()),
            ctx: args.ctx.or(cfg.ctx).unwrap_or(4096),
            temperature: args.temperature.or(cfg.temperature).unwrap_or(0.0),
            protocol_override: args.protocol,
            prompts_dir: args.prompts_dir.clone(), // not in Config
            max_ring: args.max_ring.or(cfg.max_ring),
            profile_dir: args
                .profile_dir
                .clone()
                .or(cfg.profile_dir)
                .unwrap_or_else(|| PathBuf::from("benchmarks")),
            model_key: backend_opts.model.clone(),
        });

        let ts = now_ms();
        let session = format!("api-{ts}");
        let trace_dir = state.workspace.root().join(".ferric").join("trace");
        let _ = std::fs::create_dir_all(&trace_dir);
        let trace_path = trace_dir.join(format!("{session}.jsonl"));
        let mut sink = JsonlSink::open(&trace_path, &session)
            .map_err(|e| format!("trace open: {e}"))?;

        let provider = crate::backend::create_provider(&backend_opts).await?;

        let outcome = run_with_provider(
            provider.as_ref(),
            &config.registry,
            &state.workspace,
            &config.policy,
            config.protocol,
            config.sampling.clone(),
            config.system_prompt.as_deref(),
            config.lineage.clone(),
            &mut sink,
            Some(prompt),
            Vec::new(),
            sink_fn,
            None,
            ferric_guard::TaintSet::new(),
            ferric_guard::SinkPolicy::deny(),
        )
        .await
        .map_err(|e| format!("query failed: {e}"))?;

        let stop = match &outcome.stop {
            StopReason::TaskComplete => "task_complete",
            StopReason::MaxTurns => "max_turns",
            StopReason::RepetitionGuard => "repetition",
            StopReason::NoProgress => "no_progress",
            StopReason::RepeatedFailure => "repeated_failure",
            _ => "unknown",
        };

        Ok(QueryResponse {
            text: outcome.final_text.clone(),
            turns: outcome.turns,
            stop_reason: stop.to_string(),
        })
    }

    /// Entry point for `ferric api`.
    pub async fn run_api(args: ApiArgs) -> std::process::ExitCode {
        let workspace_root = args
            .workspace
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let workspace = match Workspace::new(&workspace_root) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("workspace error: {e}");
                return std::process::ExitCode::FAILURE;
            }
        };

        let bind_addr = format!("{}:{}", args.host, args.port);

        let state = Arc::new(AppState { workspace, args });

        let app = Router::new()
            .route("/health", get(health))
            .route("/v1/query", post(query_handler))
            .route("/v1/query/stream", post(query_stream_handler))
            .with_state(state);

        eprintln!("Ferric API listening on http://{bind_addr}");
        eprintln!("  GET  /health          — health check");
        eprintln!("  POST /v1/query        — run agentic query (JSON response)");
        eprintln!("  POST /v1/query/stream  — run agentic query (SSE stream)");

        let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("bind failed: {e}");
                return std::process::ExitCode::FAILURE;
            }
        };

        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("server error: {e}");
            return std::process::ExitCode::FAILURE;
        }

        std::process::ExitCode::SUCCESS
    }
}

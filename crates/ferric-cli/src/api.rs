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
    use ferric_provider::StreamDelta;

    use crate::backend::BackendOpts;
    use crate::query::{
        HarnessPolicyArg, ProtocolArg, RunConfigArgs, build_run_config,
        ensure_supported_harness_policy, route_files, run_with_provider,
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

        /// Autonomous harness policy for every request.
        #[arg(long, value_enum)]
        pub harness_policy: Option<HarnessPolicyArg>,

        /// Directory of prompt elements.
        #[arg(long)]
        pub prompts_dir: Option<PathBuf>,

        /// Run at this tier regardless of size or measured level (ADR-098).
        #[arg(long, value_enum)]
        pub tier: Option<crate::query::TierArg>,

        /// Cap the active tool ring.
        #[arg(long)]
        pub max_ring: Option<u8>,

        /// Directory holding `model_profiles.json`.
        #[arg(long)]
        pub profile_dir: Option<PathBuf>,

        /// Declare the model's accepted non-text modalities (comma list:
        /// `image,audio,video`). Applies to every request's `files`.
        #[arg(long)]
        pub modality: Option<String>,

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
    #[derive(Debug, Clone, Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    pub struct QueryResponse {
        pub text: Option<String>,
        pub turns: u32,
        pub stop_reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub needs_input: Option<ferric_loop::NeedsInput>,
    }

    /// SSE event wrapper for streaming.
    #[derive(Debug, Serialize)]
    #[allow(dead_code)]
    pub struct SsePayload {
        pub text: Option<String>,
        pub name: Option<String>,
    }

    #[derive(Debug)]
    enum ApiQueryError {
        InvalidRequest(String),
        InvalidAttachment(String),
        InvalidContinuation(String),
        Internal(String),
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
            (Some(prompt), None, None) if !prompt.trim().is_empty() => {
                Ok(RequestMode::New { prompt })
            }
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
        let outcome = run_query(&state, &req, false, None)
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

            match run_query(&state, &request, true, Some(&sink_fn)).await {
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
        _stream: bool,
        sink_fn: Option<&(dyn Fn(StreamDelta) + Sync)>,
    ) -> Result<QueryResponse, ApiQueryError> {
        let mode = request_mode(request)?;
        let args = &state.args;
        let (backend_opts, config) = api_run_config(&state.workspace, args);

        let trace_dir = ferric_trace::trace_dir(state.workspace.root());
        let (effective_prompt, media, resume, answer) = match mode {
            RequestMode::New { prompt } => {
                let file_paths: Vec<PathBuf> = request.files.iter().map(PathBuf::from).collect();
                let declared =
                    ferric_core::parse_modalities(args.modality.as_deref().unwrap_or(""));
                let (media, prompt_suffix) = route_files(
                    &state.workspace,
                    &file_paths,
                    &declared,
                    config.caps.supports_media,
                )
                .map_err(|e| ApiQueryError::InvalidAttachment(format!("files: {e}")))?;
                let prompt = if prompt_suffix.is_empty() {
                    prompt.to_string()
                } else {
                    format!("{prompt}{prompt_suffix}")
                };
                (Some(prompt), media, None, None)
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
        ensure_supported_harness_policy(config.harness_policy)
            .map_err(ApiQueryError::InvalidRequest)?;

        let (_session, _trace_path, mut sink) = crate::query::create_trace_sink(&trace_dir, "api")
            .map_err(|e| ApiQueryError::Internal(format!("trace open: {e}")))?;

        let provider: Box<dyn ferric_provider::Provider> = if args.mock {
            Box::new(crate::query::mock_provider(config.protocol))
        } else {
            crate::backend::create_provider(&backend_opts)
                .await
                .map_err(ApiQueryError::Internal)?
        };

        let setup = crate::query::LoopSetup {
            registry: &config.registry,
            workspace: &state.workspace,
            policy: &config.policy,
            protocol: config.protocol,
            harness_policy: config.harness_policy,
            sampling: config.sampling.clone(),
            system_prompt: config.system_prompt.as_deref(),
            lineage: config.lineage.clone(),
            media,
            stream_sink: sink_fn,
            resume,
            answer,
            provenance: ferric_guard::Provenance::Clean,
            sink_policy: ferric_guard::SinkPolicy::deny(),
            hooks: None,
            edit_approver: None,
        };
        let outcome = run_with_provider(
            setup.into_run_args(provider.as_ref(), None),
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
    ) -> (BackendOpts, crate::query::RunConfig) {
        let loaded_config = crate::config::load_layered(workspace.root());
        let cfg = loaded_config.config;
        let backend_opts = crate::config::merge_backend_opts(args.backend_opts.clone(), &cfg);

        let config = build_run_config(&RunConfigArgs {
            // The HTTP caller is not necessarily the workspace owner, so a
            // workspace-level allowlist is not evidence that *this* requester
            // authorized anything. Skills stay off here until the API has a notion
            // of who is asking (ADR-091).
            workspace_root: workspace.root().to_path_buf(),
            requested_skills: Vec::new(),
            allowed_skills: Vec::new(),
            mock: args.mock,
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
            harness_policy: args.harness_policy.map(Into::into).or(cfg.harness_policy),
            prompts_dir: args.prompts_dir.clone(), // not in Config
            max_ring: args.max_ring.or(cfg.max_ring),
            tier_override: args.tier.or(cfg.tier).map(Into::into),
            profile_dir: args
                .profile_dir
                .clone()
                .or(cfg.profile_dir)
                .unwrap_or_else(|| PathBuf::from("benchmarks")),
            model_key: backend_opts.model.clone(),
            hooks: None,
        });
        (backend_opts, config)
    }

    /// Entry point for `ferric api`.
    pub async fn run_api(args: ApiArgs) -> std::process::ExitCode {
        if !is_loopback_host(&args.host) {
            eprintln!(
                "refusing unauthenticated API bind to '{}'; use 127.0.0.1, localhost, or ::1",
                args.host
            );
            return std::process::ExitCode::FAILURE;
        }

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

        // API policy is launch-time fixed. Refuse unsupported CLI or layered
        // config selections before binding a socket; the request-boundary
        // check remains as defense in depth if config changes while running.
        let launch_config = crate::config::load_layered(&workspace_root).config;
        let launch_harness_policy = args
            .harness_policy
            .map(Into::into)
            .or(launch_config.harness_policy);
        if let Err(error) = ensure_supported_harness_policy(launch_harness_policy) {
            eprintln!("api: {error}");
            return std::process::ExitCode::FAILURE;
        }

        let bind_addr = if args.host.contains(':') {
            format!("[{}]:{}", args.host, args.port)
        } else {
            format!("{}:{}", args.host, args.port)
        };

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

    fn is_loopback_host(host: &str) -> bool {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|addr| addr.is_loopback())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn api_args(workspace: &std::path::Path) -> ApiArgs {
            ApiArgs {
                workspace: Some(workspace.to_path_buf()),
                host: "127.0.0.1".to_string(),
                port: 0,
                backend_opts: BackendOpts {
                    model: None,
                    api_base: None,
                    api_key: None,
                },
                params_b: None,
                quant: None,
                family: None,
                ctx: None,
                temperature: None,
                protocol: None,
                harness_policy: None,
                prompts_dir: None,
                tier: None,
                max_ring: None,
                profile_dir: None,
                modality: None,
                mock: true,
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
            explicit.harness_policy = Some(HarnessPolicyArg::EvidencePlanner);
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
        fn backend_surface_policy_propagation() {
            let cli_workspace = tempfile::tempdir().unwrap();
            let workspace = Workspace::new(cli_workspace.path()).unwrap();
            let mut explicit = api_args(cli_workspace.path());
            explicit.harness_policy = Some(HarnessPolicyArg::Evidence);
            let (_, config) = api_run_config(&workspace, &explicit);
            assert_eq!(
                config.harness_policy,
                Some(ferric_core::HarnessPolicy::Evidence)
            );
            assert!(!cli_workspace.path().join(".ferric/trace").exists());

            let config_workspace = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(config_workspace.path().join(".ferric")).unwrap();
            std::fs::write(
                config_workspace.path().join(".ferric/config.toml"),
                "harness_policy = \"evidence\"\n",
            )
            .unwrap();
            let workspace = Workspace::new(config_workspace.path()).unwrap();
            let (_, config) = api_run_config(&workspace, &api_args(config_workspace.path()));
            assert_eq!(
                config.harness_policy,
                Some(ferric_core::HarnessPolicy::Evidence)
            );
            assert!(!config_workspace.path().join(".ferric/trace").exists());
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
}

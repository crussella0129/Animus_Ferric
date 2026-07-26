use clap::{Args, ValueEnum};
use serde::{Deserialize, Serialize};

#[cfg(feature = "backend-openai")]
use ferric_provider::Provider;

/// `Serialize`/`Deserialize` + kebab-case (T-3801): lets `Config::backend`
/// (sprint 38's persistent config) round-trip through TOML using the same
/// spelling clap's own `ValueEnum` already uses (`"openai"`).
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackendArg {
    /// OpenAI-compatible HTTP valve (llama.cpp / Ollama). Enforces a
    /// `response_format` constraint server-side → `ConstrainedJson`.
    Openai,
}

#[derive(Args, Clone)]
pub struct BackendOpts {
    /// Which backend to use. Default "openai" when neither this flag nor a
    /// config file's `backend` is set (T-3803/T-3805) — bare `Option<T>`
    /// rather than a clap default so a config-only-set value isn't masked by
    /// an indistinguishable clap default.
    #[arg(long, value_enum)]
    pub backend: Option<BackendArg>,

    /// The model string identifier (required for openai backend)
    #[arg(long)]
    pub model: Option<String>,

    /// OpenAI-compatible API base URL. Defaults to the running `ferric server`
    /// (`.ferric/server.json` in the cwd), else `http://localhost:1234/v1`.
    #[arg(long)]
    pub api_base: Option<String>,

    /// The API key for the OpenAI-compatible API (for openai backend)
    #[arg(long)]
    pub api_key: Option<String>,
}

/// Resolve the OpenAI base URL (T-805 auto-discovery): an explicit `--api-base`
/// wins, else the running `ferric server` runfile's `base_url`, else the
/// built-in default.
#[cfg(any(feature = "backend-openai", test))]
fn resolve_base(explicit: Option<&str>, runfile: Option<&str>) -> String {
    explicit
        .or(runfile)
        .map(str::to_string)
        .unwrap_or_else(|| "http://localhost:1234/v1".to_string())
}

#[cfg(feature = "backend-openai")]
pub async fn create_provider(
    opts: &BackendOpts,
) -> Result<Box<dyn Provider + Send + Sync>, String> {
    match opts.backend.unwrap_or(BackendArg::Openai) {
        BackendArg::Openai => {
            #[cfg(feature = "backend-openai")]
            {
                use ferric_provider::openai::{OpenAiConfig, OpenAiProvider};
                let model_id = opts.model.clone().unwrap_or_else(|| "default".to_string());
                let api_key = opts
                    .api_key
                    .clone()
                    .or_else(|| std::env::var("OPENAI_API_KEY").ok());
                let runfile = std::env::current_dir()
                    .ok()
                    .and_then(|d| crate::server::read_runfile(&d));
                let base_url = resolve_base(
                    opts.api_base.as_deref(),
                    runfile.as_ref().map(|r| r.base_url.as_str()),
                );
                let config = OpenAiConfig {
                    base_url,
                    api_key: api_key.unwrap_or_else(|| "ollama".to_string()),
                    model: model_id,
                };
                let provider = OpenAiProvider::new(config);
                Ok(Box::new(provider))
            }
        }
    }
}

// No `create_provider` stub for the backend-free build: the callers carry
// their own `cfg(not(...))` stubs that surface `BACKEND_FEATURE_MISSING`
// directly, so a stub here would be unreachable dead code.
//
// There used to be a `#[cfg(not(feature = "backend-openai"))]` arm inside
// `create_provider` returning "binary built without openai backend". The whole
// function is already gated on that feature, so the arm could never compile in
// and that message could never be produced — it read as a handled case while
// contradicting the comment above. Removed sprint 103 (ADR-094).

/// What a caller is told when the binary has no backend compiled in. One
/// definition: this text lived in three byte-identical copies (`chat.rs`,
/// `icm.rs`, `mcp.rs`), each unreachable in a normally-built binary and so
/// each free to drift unnoticed. `ferric dream` deliberately keeps its own
/// wording — it has no `--mock`, so pointing at one would be wrong.
// Referenced only from `cfg(not(feature = "backend-openai"))` paths and the
// test below, so a normal feature-on build sees no use of it. That is the
// point — this is the message for the build that cannot reach those paths.
#[allow(dead_code)]
pub(crate) const BACKEND_FEATURE_MISSING: &str = "this binary was built without backend features; \
     rebuild with `cargo build --features backend-openai`, or use --mock";

/// A provider plus the runtime that drives it — the pair every real-backend
/// caller needs, and the two lines each of them used to write.
#[cfg(feature = "backend-openai")]
pub(crate) fn create_provider_with_runtime(
    opts: &BackendOpts,
) -> Result<(Box<dyn Provider + Send + Sync>, tokio::runtime::Runtime), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("tokio runtime: {e}"))?;
    let provider = runtime.block_on(create_provider(opts))?;
    Ok((provider, runtime))
}

/// A backend a command has finished resolving: either the scripted mock, or a
/// real provider owning the ONE runtime it will be driven on.
///
/// `chat.rs` and `icm.rs` each declared this enum and its constructor
/// identically before sprint 103 (ADR-094). `mcp.rs` keeps a shape of its own
/// on purpose — see `mcp::Executor`.
pub(crate) enum ResolvedBackend {
    Mock,
    #[cfg(feature = "backend-openai")]
    Real {
        provider: Box<dyn Provider + Send + Sync>,
        runtime: tokio::runtime::Runtime,
    },
}

impl ResolvedBackend {
    /// Resolve a real backend, or explain why this binary cannot.
    #[cfg(feature = "backend-openai")]
    pub(crate) fn real(opts: &BackendOpts) -> Result<Self, String> {
        let (provider, runtime) = create_provider_with_runtime(opts)?;
        Ok(ResolvedBackend::Real { provider, runtime })
    }

    #[cfg(not(feature = "backend-openai"))]
    pub(crate) fn real(_opts: &BackendOpts) -> Result<Self, String> {
        Err(BACKEND_FEATURE_MISSING.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one place this string can be checked: it is a user-facing error in
    /// a path a normally-built binary cannot reach, and it went three sprints
    /// as three untested copies. It has to name the feature to rebuild with
    /// *and* the way to proceed without one — a diagnostic that says only
    /// "unsupported" leaves the user nowhere.
    #[test]
    fn the_backend_feature_diagnostic_names_the_feature_and_the_alternative() {
        assert!(BACKEND_FEATURE_MISSING.contains("backend-openai"));
        assert!(BACKEND_FEATURE_MISSING.contains("--mock"));
    }

    #[test]
    fn api_base_precedence() {
        // explicit > runfile > built-in default.
        assert_eq!(
            resolve_base(Some("http://explicit/v1"), Some("http://runfile/v1")),
            "http://explicit/v1"
        );
        assert_eq!(
            resolve_base(None, Some("http://runfile/v1")),
            "http://runfile/v1"
        );
        assert_eq!(resolve_base(None, None), "http://localhost:1234/v1");
    }
}

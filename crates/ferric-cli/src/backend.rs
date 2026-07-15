use clap::{Args, ValueEnum};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
                let model_id = opts
                    .model
                    .clone()
                    .unwrap_or_else(|| "default".to_string());
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
            #[cfg(not(feature = "backend-openai"))]
            {
                Err("binary built without openai backend".to_string())
            }
        }
    }
}

// No `create_provider` stub for the backend-free build: the only callers
// (`query::drive_real`, `toolbench`) carry their own `cfg(not(any(...)))`
// stubs that surface the "built without backends; use --mock" error directly,
// so a stub here would be unreachable dead code.

#[cfg(test)]
mod tests {
    use super::*;

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

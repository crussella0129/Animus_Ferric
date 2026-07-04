use clap::{Args, ValueEnum};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// `Provider` is only named by the feature-gated `create_provider` return type.
#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai"))]
use ferric_provider::Provider;

/// `Serialize`/`Deserialize` + kebab-case (T-3801): lets `Config::backend`
/// (sprint 38's persistent config) round-trip through TOML using the same
/// spelling clap's own `ValueEnum` already uses (`"mistral"`/`"openai"`).
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackendArg {
    /// In-process mistral.rs (GGUF). Text-only / `TextXml` — no constraint.
    Mistral,
    /// OpenAI-compatible HTTP valve (llama.cpp / Ollama). Enforces a
    /// `response_format` constraint server-side → `ConstrainedJson`.
    Openai,
}

#[derive(Args, Clone)]
pub struct BackendOpts {
    /// Which backend to use
    #[arg(long, value_enum, default_value = "mistral")]
    pub backend: BackendArg,

    /// Directory containing the GGUF model (required for the mistral backend)
    #[arg(long)]
    pub model_dir: Option<PathBuf>,

    /// GGUF file name inside --model-dir (required for mistral backend)
    #[arg(long)]
    pub model_file: Option<String>,

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

    /// Path to a chat template override (for GGUFs without an embedded one)
    #[arg(long)]
    pub chat_template: Option<PathBuf>,

    /// Path to the model's real tokenizer.json. REQUIRED for `--protocol
    /// grammar` on GGUF models. Also read from FERRIC_TOKENIZER_JSON.
    #[arg(long)]
    pub tokenizer_json: Option<PathBuf>,

    /// Alternatively, an HF model id to source tokenizer.json from.
    #[arg(long)]
    pub tok_model_id: Option<String>,
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

#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai"))]
pub async fn create_provider(
    opts: &BackendOpts,
) -> Result<Box<dyn Provider + Send + Sync>, String> {
    match opts.backend {
        BackendArg::Mistral => {
            #[cfg(feature = "backend-mistralrs")]
            {
                use ferric_provider::mistralrs::{MistralRsConfig, MistralRsProvider};

                let model_dir = opts
                    .model_dir
                    .as_ref()
                    .ok_or("--model-dir is required for mistral backend")?;
                let model_file = opts
                    .model_file
                    .as_ref()
                    .ok_or("--model-file is required for mistral backend")?;

                let tokenizer_json = opts
                    .tokenizer_json
                    .clone()
                    .or_else(|| std::env::var_os("FERRIC_TOKENIZER_JSON").map(PathBuf::from));

                if opts.tok_model_id.is_none() {
                    unsafe {
                        std::env::set_var("HF_HUB_OFFLINE", "1");
                    }
                }

                let mut config = MistralRsConfig::new(model_dir, model_file);
                config.chat_template = opts.chat_template.as_ref().map(|p| p.display().to_string());
                config.tokenizer_json = tokenizer_json;
                config.tok_model_id = opts.tok_model_id.clone();

                let provider = MistralRsProvider::load(config)
                    .await
                    .map_err(|e| format!("mistral backend: {e}"))?;
                Ok(Box::new(provider))
            }
            #[cfg(not(feature = "backend-mistralrs"))]
            {
                Err("binary built without mistralrs backend".to_string())
            }
        }
        BackendArg::Openai => {
            #[cfg(feature = "backend-openai")]
            {
                use ferric_provider::openai::{OpenAiConfig, OpenAiProvider};
                let model_id = opts
                    .model
                    .clone()
                    .ok_or("--model is required for openai backend")?;
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

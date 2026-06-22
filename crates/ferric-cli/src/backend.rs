use std::path::PathBuf;
use clap::{Args, ValueEnum};
use ferric_provider::Provider;

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum BackendArg {
    Mistral,
    Openai,
    Python,
}

#[derive(Args, Clone)]
pub struct BackendOpts {
    /// Which backend to use
    #[arg(long, value_enum, default_value = "mistral")]
    pub backend: BackendArg,

    /// Directory containing the model (required for mistral and python backends)
    #[arg(long)]
    pub model_dir: Option<PathBuf>,

    /// GGUF file name inside --model-dir (required for mistral backend)
    #[arg(long)]
    pub model_file: Option<String>,

    /// The model string identifier (required for openai backend)
    #[arg(long)]
    pub model: Option<String>,

    /// The OpenAI-compatible API base URL (for openai backend)
    #[arg(long, default_value = "http://localhost:1234/v1")]
    pub api_base: String,

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

#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai", feature = "backend-python"))]
pub async fn create_provider(opts: &BackendOpts) -> Result<Box<dyn Provider + Send + Sync>, String> {
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
                use ferric_provider::openai::{OpenAiProvider, OpenAiConfig};
                let model_id = opts.model.clone().ok_or("--model is required for openai backend")?;
                let api_key = opts.api_key.clone().or_else(|| std::env::var("OPENAI_API_KEY").ok());
                let config = OpenAiConfig {
                    base_url: opts.api_base.clone(),
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
        BackendArg::Python => {
            #[cfg(feature = "backend-python")]
            {
                use ferric_provider::python::{PythonProvider, PythonConfig};
                let model_dir = opts
                    .model_dir
                    .as_ref()
                    .ok_or("--model-dir is required for python backend")?;
                
                let config = PythonConfig {
                    model_dir: model_dir.clone(),
                };
                let provider = PythonProvider::new(config);
                Ok(Box::new(provider))
            }
            #[cfg(not(feature = "backend-python"))]
            {
                Err("binary built without python backend".to_string())
            }
        }
    }
}

#[cfg(not(any(feature = "backend-mistralrs", feature = "backend-openai", feature = "backend-python")))]
pub async fn create_provider(_opts: &BackendOpts) -> Result<Box<dyn Provider + Send + Sync>, String> {
    Err(
        "this binary was built without backend features; \
         rebuild with `cargo build --features backend-mistralrs,backend-openai,backend-python`, or use --mock"
            .to_string(),
    )
}

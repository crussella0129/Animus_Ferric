//! `ferric query` — flags defined in T-110, handler wired in T-111.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;

#[derive(Args)]
pub struct QueryArgs {
    /// The task prompt
    pub prompt: String,

    /// Workspace root (containment boundary). Default: current directory.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Directory containing the GGUF model
    #[arg(long)]
    pub model_dir: Option<PathBuf>,

    /// GGUF file name inside --model-dir
    #[arg(long)]
    pub model_file: Option<String>,

    /// Context window in tokens (ModelProfile is config-supplied, ADR-006)
    #[arg(long, default_value_t = 4096)]
    pub ctx: u32,

    /// Parameter count in billions
    #[arg(long, default_value_t = 1.2)]
    pub params_b: f32,

    /// Quantization label
    #[arg(long, default_value = "Q4_K_M")]
    pub quant: String,

    /// Model family label
    #[arg(long, default_value = "unknown")]
    pub family: String,

    /// Path to a chat template override (for GGUFs without an embedded one)
    #[arg(long)]
    pub chat_template: Option<PathBuf>,

    /// Sampling temperature (0.0 selects the deterministic sampler)
    #[arg(long, default_value_t = 0.0)]
    pub temperature: f32,

    /// Run against a built-in scripted mock instead of a real model
    #[arg(long)]
    pub mock: bool,
}

pub fn run_query(_args: QueryArgs) -> ExitCode {
    eprintln!("query: handler lands in T-111");
    ExitCode::FAILURE
}

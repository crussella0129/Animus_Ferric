//! `ferric setup`, and the same offer made by a first `ferric` run
//! (sprint 128, INT-0016).
//!
//! A local model needs an engine (llama.cpp's `llama-server`) and a GGUF
//! file. When either is missing, Ferric says exactly what it would fetch, from
//! where, how large it is and under which license, then asks once. Everything
//! comes from a pinned catalog and is checked against a SHA-256 before use.

use std::io::{IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use clap::Args;
use ferric_setup::catalog::{HUGGING_FACE, MODELS, ModelEntry, find_model};
use ferric_setup::engine::{self, GITHUB_API};
use ferric_setup::{Hardware, LLAMA_CPP_TAG};

#[derive(Args, Default)]
pub struct SetupArgs {
    /// Download without asking (for scripts and installers)
    #[arg(long, short = 'y')]
    pub yes: bool,
    /// Model to install: 0.5b, 1.5b, 7b or 14b. Default: the one that fits this machine
    #[arg(long)]
    pub model: Option<String>,
    /// Folder for models. Default: FERRIC_MODELS_DIR, the configured folder, ./models if it exists, else Ferric's data folder
    #[arg(long)]
    pub models_dir: Option<PathBuf>,
    /// Show the catalog and what this machine would get; download nothing
    #[arg(long)]
    pub list: bool,
}

/// What a machine still needs.
pub(crate) struct Needs {
    pub engine: bool,
    pub model: Option<&'static ModelEntry>,
    /// The model was asked for by name, not picked for the machine.
    pub requested: bool,
    pub models_dir: PathBuf,
}

impl Needs {
    pub(crate) fn nothing(&self) -> bool {
        !self.engine && self.model.is_none()
    }
}

/// Is an engine available: `llama-server` on PATH, or one Ferric installed?
pub(crate) fn engine_available() -> bool {
    crate::startup::find_engine().is_ok()
}

fn has_gguf(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("gguf"))
                && entry.file_type().is_ok_and(|kind| kind.is_file())
        })
    })
}

/// Work out what is missing. A requested model is always offered (unless
/// already present); otherwise a model is offered only when the folder holds
/// no GGUF at all.
pub(crate) fn plan(
    models_dir: &Path,
    requested: Option<&'static ModelEntry>,
    hardware: &Hardware,
) -> Needs {
    let model = match requested {
        Some(model) if models_dir.join(model.file).is_file() => None,
        Some(model) => Some(model),
        None if has_gguf(models_dir) => None,
        None => Some(hardware.recommended_model()),
    };
    Needs {
        engine: !engine_available(),
        model,
        requested: requested.is_some(),
        models_dir: models_dir.to_path_buf(),
    }
}

/// The text shown before asking.
pub(crate) fn describe(needs: &Needs, hardware: &Hardware) -> String {
    let mut text = String::from("To run a model on this machine, Ferric needs:\n");
    if needs.engine {
        let asset = engine::asset_name(LLAMA_CPP_TAG, hardware.os, hardware.arch)
            .unwrap_or_else(|error| error.to_string());
        text.push_str(&format!(
            "  • the llama.cpp engine, release {LLAMA_CPP_TAG} ({asset}), from github.com/ggml-org/llama.cpp\n"
        ));
    }
    if let Some(model) = needs.model {
        let why = if needs.requested {
            "as requested".to_string()
        } else {
            format!("chosen for this machine ({})", hardware.memory_label())
        };
        text.push_str(&format!(
            "  • {} (Q4_K_M, {:.1} GB, {}), from huggingface.co/{}\n    {why}; others: ferric setup --model 0.5b | 1.5b | 7b | 14b\n",
            model.label,
            model.size_gb(),
            model.license,
            model.repo,
        ));
    }
    let data = ferric_setup::paths::data_dir()
        .map(|dir| dir.display().to_string())
        .unwrap_or_else(|| "Ferric's data folder".to_string());
    text.push_str(&format!(
        "Each file is pinned and checked against its SHA-256 before use.\nEngine → {data}/engine   Model → {}\n",
        needs.models_dir.display()
    ));
    text
}

/// Resolves when `flag` is set (never, without a flag).
async fn until_cancelled(flag: Option<&AtomicBool>) {
    let Some(flag) = flag else {
        return std::future::pending().await;
    };
    while !flag.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

const CANCELLED: &str =
    "Cancelled. The partial download is kept and resumes the next time you run ferric.";

/// Download what is missing, printing progress to stderr. Setting `cancel`
/// (Ctrl-C in a session) stops the download; its part file is kept.
pub(crate) fn execute(
    needs: &Needs,
    runtime: &tokio::runtime::Runtime,
    cancel: Option<&AtomicBool>,
) -> Result<(), String> {
    let client = ferric_setup::fetch::client().map_err(|e| e.to_string())?;
    let hardware = Hardware::detect();
    if needs.engine {
        let root = ferric_setup::paths::engine_root()
            .ok_or("Cannot find a home folder for Ferric's data. Set FERRIC_DATA_DIR.")?;
        let api = std::env::var("FERRIC_ENGINE_API_BASE").unwrap_or_else(|_| GITHUB_API.into());
        let mut bar = Bar::new("engine");
        let mut on_progress = |done, total| bar.update(done, total);
        let installed = runtime
            .block_on(async {
                tokio::select! {
                    result = engine::install(
                        &client,
                        &api,
                        LLAMA_CPP_TAG,
                        hardware.os,
                        hardware.arch,
                        &root,
                        &mut on_progress,
                    ) => Some(result),
                    () = until_cancelled(cancel) => None,
                }
            })
            .ok_or(CANCELLED)?
            .map_err(|e| e.to_string())?;
        bar.finish("verified");
        eprintln!("  engine ready: {}", installed.server.display());
    }
    if let Some(model) = needs.model {
        let base = std::env::var("FERRIC_MODEL_BASE_URL").unwrap_or_else(|_| HUGGING_FACE.into());
        let dest = needs.models_dir.join(model.file);
        let mut bar = Bar::new("model");
        let url = model.url(&base);
        let mut on_progress = |done, total| bar.update(done, total);
        runtime
            .block_on(async {
                tokio::select! {
                    result = ferric_setup::fetch::download_verified(
                        &client,
                        &url,
                        &dest,
                        model.sha256,
                        Some(model.bytes),
                        model.label,
                        &mut on_progress,
                    ) => Some(result),
                    () = until_cancelled(cancel) => None,
                }
            })
            .ok_or(CANCELLED)?
            .map_err(|e| e.to_string())?;
        bar.finish("verified");
        eprintln!("  model ready: {}", dest.display());
    }
    Ok(())
}

/// The first-run hook for `ferric`: when something is missing, describe it,
/// ask once (Enter means yes), and fetch it. Returns `false` when the person
/// declined. A configured endpoint or an explicit model file skips all of this.
pub(crate) fn offer_if_needed(
    models_dir: &Path,
    skip: bool,
    interactive: bool,
    say: &dyn Fn(&str),
    ask: &dyn Fn(&str) -> Result<Option<String>, String>,
    runtime: &tokio::runtime::Runtime,
    cancel: &AtomicBool,
) -> Result<bool, String> {
    if skip {
        return Ok(true);
    }
    let hardware = Hardware::detect();
    let needs = plan(models_dir, None, &hardware);
    if needs.nothing() {
        return Ok(true);
    }
    if !interactive {
        return Err(format!(
            "{}Run `ferric setup --yes` to download them, then run again.",
            describe(&needs, &hardware)
        ));
    }
    say(&describe(&needs, &hardware));
    let answer = ask("Download now? [Y/n] ")?;
    if !accepts(answer.as_deref()) {
        say("Nothing was downloaded. Run `ferric setup` whenever you are ready.");
        return Ok(false);
    }
    execute(&needs, runtime, Some(cancel))?;
    say("Ready.");
    Ok(true)
}

/// Enter (or y/yes) accepts; anything else declines.
fn accepts(answer: Option<&str>) -> bool {
    matches!(
        answer.map(|a| a.trim().to_ascii_lowercase()).as_deref(),
        Some("" | "y" | "yes")
    )
}

/// `ferric setup`.
pub fn run_setup(args: SetupArgs) -> ExitCode {
    let hardware = Hardware::detect();
    if args.list {
        let recommended = hardware.recommended_model();
        println!(
            "This machine: {} {}, {}",
            hardware.os,
            hardware.arch,
            hardware.memory_label()
        );
        println!(
            "Engine: llama.cpp {LLAMA_CPP_TAG} ({})",
            engine::asset_name(LLAMA_CPP_TAG, hardware.os, hardware.arch)
                .unwrap_or_else(|e| e.to_string())
        );
        for model in MODELS {
            println!(
                "  {:<5} {:<30} {:>5.1} GB  {}{}",
                model.id,
                model.label,
                model.size_gb(),
                model.license,
                if model.id == recommended.id {
                    "  ← recommended"
                } else {
                    ""
                }
            );
        }
        return ExitCode::SUCCESS;
    }
    let requested = match args.model.as_deref() {
        Some(id) => match find_model(id) {
            Some(model) => Some(model),
            None => {
                eprintln!(
                    "setup: unknown model '{id}'. Choose 0.5b, 1.5b, 7b or 14b (see `ferric setup --list`)."
                );
                return ExitCode::from(2);
            }
        },
        None => None,
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let configured = crate::config::load_layered(&cwd)
        .ok()
        .and_then(|loaded| loaded.config.models_dir);
    let models_dir = crate::startup::resolve_models_dir(
        args.models_dir.as_deref(),
        &|key| std::env::var(key).ok(),
        configured.as_deref(),
        &cwd,
    );
    let needs = plan(&models_dir, requested, &hardware);
    if needs.nothing() {
        println!(
            "Ferric is ready: an engine is installed and {} holds a model.",
            models_dir.display()
        );
        println!("Run `ferric` in any project folder.");
        return ExitCode::SUCCESS;
    }
    println!("{}", describe(&needs, &hardware));
    if !args.yes {
        if !std::io::stdin().is_terminal() {
            eprintln!("setup: re-run with --yes to download without a prompt.");
            return ExitCode::from(2);
        }
        print!("Download now? [Y/n] ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).is_err() || !accepts(Some(&line)) {
            println!("Nothing was downloaded.");
            return ExitCode::SUCCESS;
        }
    }
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("setup: {error}");
            return ExitCode::FAILURE;
        }
    };
    match execute(&needs, &runtime, None) {
        Ok(()) => {
            println!("Ready. Run `ferric` in any project folder.");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("setup: {error}");
            ExitCode::FAILURE
        }
    }
}

/// A one-line progress display on stderr.
struct Bar {
    label: &'static str,
    started: Instant,
    last: Option<Instant>,
    tty: bool,
    last_percent: u64,
}

impl Bar {
    fn new(label: &'static str) -> Self {
        Bar {
            label,
            started: Instant::now(),
            last: None,
            tty: std::io::stderr().is_terminal(),
            last_percent: u64::MAX,
        }
    }

    fn update(&mut self, done: u64, total: Option<u64>) {
        let percent = total.map(|t| done.saturating_mul(100) / t.max(1));
        if self.tty {
            if self
                .last
                .is_some_and(|last| last.elapsed().as_millis() < 200)
            {
                return;
            }
            self.last = Some(Instant::now());
            let rate = done as f64 / self.started.elapsed().as_secs_f64().max(0.001) / 1e6;
            let line = match (percent, total) {
                (Some(p), Some(t)) => format!(
                    "  {:<6} {p:>3}%  {:.2} / {:.2} GB  {rate:.1} MB/s",
                    self.label,
                    done as f64 / 1e9,
                    t as f64 / 1e9
                ),
                _ => format!(
                    "  {:<6} {:.2} GB  {rate:.1} MB/s",
                    self.label,
                    done as f64 / 1e9
                ),
            };
            eprint!("\r{line}\x1b[K");
        } else if let Some(p) = percent
            && p / 10 != self.last_percent / 10
        {
            self.last_percent = p;
            eprintln!("  {} {p}%", self.label);
        }
    }

    fn finish(&self, word: &str) {
        if self.tty {
            eprintln!();
        }
        eprintln!(
            "  {} {word} in {:.0} s",
            self.label,
            self.started.elapsed().as_secs_f64()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_and_yes_accept_anything_else_declines() {
        assert!(accepts(Some("")));
        assert!(accepts(Some("\n")));
        assert!(accepts(Some("Y")));
        assert!(accepts(Some(" yes ")));
        assert!(!accepts(Some("n")));
        assert!(!accepts(Some("later")));
        assert!(!accepts(None));
    }

    #[test]
    fn a_folder_with_a_gguf_needs_no_model_and_a_request_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let hardware = Hardware {
            os: "linux",
            arch: "x86_64",
            total_memory: Some(16 << 30),
        };
        let empty = plan(dir.path(), None, &hardware);
        assert_eq!(empty.model.map(|m| m.id), Some("7b"));
        std::fs::write(dir.path().join("mine.GGUF"), b"x").unwrap();
        assert!(plan(dir.path(), None, &hardware).model.is_none());
        let asked = plan(dir.path(), find_model("1.5b"), &hardware);
        assert_eq!(asked.model.map(|m| m.id), Some("1.5b"));
        std::fs::write(dir.path().join(find_model("1.5b").unwrap().file), b"x").unwrap();
        assert!(
            plan(dir.path(), find_model("1.5b"), &hardware)
                .model
                .is_none()
        );
    }

    #[test]
    fn the_offer_names_source_size_license_and_destination() {
        let hardware = Hardware {
            os: "linux",
            arch: "x86_64",
            total_memory: Some(8 << 30),
        };
        let needs = Needs {
            engine: true,
            model: Some(hardware.recommended_model()),
            requested: false,
            models_dir: PathBuf::from("/models"),
        };
        let text = describe(&needs, &hardware);
        assert!(text.contains("llama.cpp engine, release"));
        assert!(text.contains("bin-ubuntu-x64.tar.gz"));
        assert!(text.contains("Qwen2.5-Coder 1.5B Instruct"));
        assert!(text.contains("Apache-2.0"));
        assert!(text.contains("SHA-256"));
        assert!(text.contains("/models"));
        assert!(text.contains("chosen for this machine (8 GiB RAM)"));
        let asked = Needs {
            requested: true,
            ..needs
        };
        assert!(describe(&asked, &hardware).contains("as requested"));
    }
}

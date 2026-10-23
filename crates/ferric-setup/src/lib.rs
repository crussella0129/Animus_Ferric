//! `ferric-setup` — get from nothing to a running local model with one yes
//! (sprint 128, INT-0016).
//!
//! A first run needs two things that are not part of the `ferric` binary: the
//! llama.cpp engine and a GGUF model. This crate decides which ones suit the
//! machine and fetches them. It touches nothing until the person has seen
//! exactly what will be downloaded and said yes.
//!
//! - [`catalog`] — the pinned choices: a llama.cpp release tag, and models
//!   fixed to a Hugging Face revision with their SHA-256, size and license.
//! - [`hardware`] — what this machine is (OS, CPU architecture, memory), and
//!   the model that fits it.
//! - [`fetch`] — streaming, resumable downloads that write nothing to the
//!   final path until the SHA-256 matches.
//! - [`engine`] — the llama.cpp asset for this platform, its digest from the
//!   GitHub release, extraction with the system `tar`, and the record later
//!   runs read to find the engine.
//! - [`paths`] — Ferric's per-user data directory, which holds the engine and
//!   models (outside every workspace).

pub mod catalog;
pub mod engine;
pub mod fetch;
pub mod hardware;
pub mod paths;

pub use catalog::{LLAMA_CPP_TAG, MODELS, ModelEntry};
pub use hardware::Hardware;

#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("{0}")]
    Network(String),
    #[error("{path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "{what} failed verification: expected SHA-256 {expected}, got {actual}. The file was discarded."
    )]
    Checksum {
        what: String,
        expected: String,
        actual: String,
    },
    #[error("{0}")]
    Unsupported(String),
    #[error("{0}")]
    Engine(String),
}

impl SetupError {
    pub(crate) fn io(path: &std::path::Path, source: std::io::Error) -> Self {
        SetupError::Io {
            path: path.display().to_string(),
            source,
        }
    }
}

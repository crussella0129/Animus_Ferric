//! `ferric-gate` — the network boundary for Ferric's local HTTP services
//! (sprint 127, INT-0015).
//!
//! Loopback is not a trust boundary. Every process on the machine can reach
//! `127.0.0.1`, and so can every web page the user opens: a cross-site form
//! POST needs no CORS preflight, and DNS rebinding turns `attacker.example`
//! into a same-origin name for `127.0.0.1`. That is how the 2026 opencode
//! advisories turned a local agent server into remote code execution.
//!
//! The gate makes Ferric's services refuse all of that by construction:
//!
//! - [`token`] — a per-service 256-bit bearer token in a mode-0600 file
//!   outside every workspace, compared in constant time.
//! - [`guard`] — the pure request check: the `Host` must name this listener
//!   (no rebinding), any browser fingerprint (`Origin`, `Sec-Fetch-*`) is
//!   refused, the token is required, and bodies must be JSON. The order is
//!   fixed and nothing is configurable at request time.
//! - [`layer`] — the axum middleware that applies the guard to a whole router
//!   and strips the credential before any handler or upstream sees it.
//! - [`bind`] — listeners bind loopback, or the address of a Ferric tunnel;
//!   never a wildcard or public interface.
//! - [`tunnel`] — a self-hosted WireGuard plan (keys, configs, UAPI) for the
//!   GotaTun userspace data plane, so remote access needs no coordination
//!   server.

pub mod bind;
pub mod guard;
pub mod layer;
pub mod token;
pub mod tunnel;

pub use bind::check_bind;
pub use guard::{Denial, Gate};
pub use layer::protect;
pub use token::{Token, default_token_path};

/// Refuse a secret file that group or others can read (Unix).
pub fn token_file_private(path: &std::path::Path) -> Result<(), GateError> {
    token::check_private(path)
}

/// Response header set on every request the gate refuses, so a client (or
/// the probe) can tell a gate denial from an application error.
pub const DENIAL_HEADER: &str = "x-ferric-gate";

#[derive(Debug, thiserror::Error)]
pub enum GateError {
    #[error("cannot draw randomness from the operating system: {0}")]
    Random(String),
    #[error("{path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "{path} is readable by other users (mode {mode:o}); run `chmod 600 {path}` or delete it to mint a new token"
    )]
    TokenFileTooOpen { path: String, mode: u32 },
    #[error("{0} does not contain a Ferric token (expected `ferric_` and 64 hex digits)")]
    MalformedToken(String),
    #[error("{0}")]
    Tunnel(String),
}

impl GateError {
    pub(crate) fn io(path: &std::path::Path, source: std::io::Error) -> Self {
        GateError::Io {
            path: path.display().to_string(),
            source,
        }
    }
}

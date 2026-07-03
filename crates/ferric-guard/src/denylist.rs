//! Compile-time deny lists. There is deliberately no runtime mutation API and
//! no config override: security policy is hardcoded (ADR-005), and the LLM is
//! never consulted.

/// Path segments that are never writable, at any permission level.
/// `.git` covers config/hooks injection; `.ferric` protects the trace from
/// the model that is being traced; the rest are credential stores.
pub const DENIED_WRITE_SEGMENTS: &[&str] =
    &[".git", ".ferric", ".ssh", ".gnupg", ".aws", ".kube", ".gpg"];

/// File names that are never writable, wherever they live.
pub const DENIED_WRITE_FILES: &[&str] = &[
    "id_rsa",
    "id_ecdsa",
    "id_ed25519",
    "authorized_keys",
    "known_hosts",
    "credentials",
];

/// Command patterns reserved for the future exec tool (s1+). Present now so
/// the policy surface is complete and reviewed from the start.
pub const DENIED_COMMAND_PATTERNS: &[&str] = &[
    "rm -rf /",
    "mkfs",
    "dd if=",
    "git push --force",
    "shutdown",
    "reboot",
];

/// Path segments that are never READABLE (sprint 35 audit). Narrower than
/// [`DENIED_WRITE_SEGMENTS`] — deliberately excludes `.git`: reading git
/// metadata/config for code context is a legitimate, common agent need, and
/// only *writes* to `.git` are dangerous (hook/config injection). These
/// segments are pure credential stores plus `.ferric` (protects the trace
/// from the model being traced, symmetric with the write-side rule).
pub const DENIED_READ_SEGMENTS: &[&str] = &[".ssh", ".gnupg", ".aws", ".kube", ".ferric"];

/// File names that are never readable, wherever they live. `DENIED_WRITE_FILES`
/// plus `.env` — the single most common real-world secret file in a coding
/// workspace (API keys, DB passwords), not previously covered by any list.
/// Read-only concern: creating/editing a `.env` is normal dev work; reading an
/// *existing* one with real secrets into the model's context (and therefore
/// the untruncated trace, ADR-002) is the risk.
pub const DENIED_READ_FILES: &[&str] = &[
    "id_rsa",
    "id_ecdsa",
    "id_ed25519",
    "authorized_keys",
    "known_hosts",
    "credentials",
    ".env",
];

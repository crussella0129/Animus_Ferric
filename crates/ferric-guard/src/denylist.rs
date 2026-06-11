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

//! Per-service bearer tokens.
//!
//! A token is 32 bytes from the operating system's CSPRNG, written as
//! `ferric_` plus 64 hex digits (the prefix makes a leaked token easy to
//! find with secret scanners). It lives in a file created mode 0600 inside a
//! mode-0700 directory under the user's state directory, never inside a
//! workspace, so the model's own file tools cannot read it.

use std::fmt;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::GateError;

pub const TOKEN_PREFIX: &str = "ferric_";

/// A bearer token. `Debug` never prints the secret.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

impl Token {
    /// Mint a fresh token from the OS CSPRNG.
    pub fn generate() -> Result<Self, GateError> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|error| GateError::Random(error.to_string()))?;
        Ok(Token(format!("{TOKEN_PREFIX}{}", hex::encode(bytes))))
    }

    /// Parse a token from text (surrounding whitespace is ignored).
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let hex_part = text.strip_prefix(TOKEN_PREFIX)?;
        (hex_part.len() == 64 && hex_part.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| Token(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// SHA-256 of the token. The gate keeps only this digest.
    pub fn digest(&self) -> [u8; 32] {
        Sha256::digest(self.0.as_bytes()).into()
    }

    /// Read the token at `path`, or mint one and create the file (mode 0600,
    /// parent mode 0700) when it does not exist. Returns whether it was
    /// created. An existing file readable by group or others is refused.
    pub fn load_or_create(path: &Path) -> Result<(Self, bool), GateError> {
        match fs::read_to_string(path) {
            Ok(text) => {
                check_private(path)?;
                let token = Token::parse(&text)
                    .ok_or_else(|| GateError::MalformedToken(path.display().to_string()))?;
                Ok((token, false))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let token = Token::generate()?;
                write_private(path, token.as_str())?;
                Ok((token, true))
            }
            Err(error) => Err(GateError::io(path, error)),
        }
    }
}

/// Where a service's token lives: `$FERRIC_STATE_DIR/gate/<service>.token`,
/// else `$XDG_STATE_HOME/ferric/...` or `~/.local/state/ferric/...` on Unix
/// and `%LOCALAPPDATA%\ferric\...` on Windows.
pub fn default_token_path(service: &str) -> Option<PathBuf> {
    state_dir().map(|dir| dir.join("gate").join(format!("{service}.token")))
}

/// Ferric's per-user state directory (see [`default_token_path`]).
pub fn state_dir() -> Option<PathBuf> {
    let env = |name: &str| {
        std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    if let Some(dir) = env("FERRIC_STATE_DIR") {
        return Some(dir);
    }
    if cfg!(windows) {
        env("LOCALAPPDATA").map(|dir| dir.join("ferric"))
    } else {
        env("XDG_STATE_HOME")
            .or_else(|| env("HOME").map(|home| home.join(".local").join("state")))
            .map(|dir| dir.join("ferric"))
    }
}

/// Create `path` exclusively with owner-only permissions and write `text`.
pub(crate) fn write_private(path: &Path, text: &str) -> Result<(), GateError> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        create_private_dir(parent)?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| GateError::io(path, e))?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|e| GateError::io(path, e))
}

fn create_private_dir(dir: &Path) -> Result<(), GateError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(dir).map_err(|e| GateError::io(dir, e))
}

/// Refuse a secret file that group or others can read (Unix only; on
/// Windows the per-user profile directory's ACL is the boundary).
pub(crate) fn check_private(path: &Path) -> Result<(), GateError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = fs::metadata(path)
            .map_err(|e| GateError::io(path, e))?
            .permissions()
            .mode()
            & 0o777;
        if mode & 0o077 != 0 {
            return Err(GateError::TokenFileTooOpen {
                path: path.display().to_string(),
                mode,
            });
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_are_well_formed_and_unique() {
        let a = Token::generate().unwrap();
        let b = Token::generate().unwrap();
        assert_ne!(a, b);
        assert!(Token::parse(a.as_str()).is_some());
        assert_eq!(a.as_str().len(), TOKEN_PREFIX.len() + 64);
    }

    #[test]
    fn debug_never_prints_the_secret() {
        let token = Token::generate().unwrap();
        assert!(!format!("{token:?}").contains(&token.as_str()[8..]));
    }

    #[test]
    fn parse_rejects_wrong_shapes() {
        assert!(Token::parse("ferric_abc").is_none());
        assert!(Token::parse(&format!("other_{}", "a".repeat(64))).is_none());
        assert!(Token::parse(&format!("ferric_{}", "g".repeat(64))).is_none());
        assert!(Token::parse(&format!("  ferric_{}\n", "a".repeat(64))).is_some());
    }

    #[test]
    fn load_or_create_mints_once_then_reuses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gate").join("api.token");
        let (first, created) = Token::load_or_create(&path).unwrap();
        assert!(created);
        let (second, created_again) = Token::load_or_create(&path).unwrap();
        assert!(!created_again);
        assert_eq!(first, second);
    }

    #[cfg(unix)]
    #[test]
    fn token_file_is_owner_only_and_open_files_are_refused() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gate").join("valve.token");
        Token::load_or_create(&path).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dir_mode = fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);

        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let error = Token::load_or_create(&path).unwrap_err();
        assert!(matches!(error, GateError::TokenFileTooOpen { .. }));
    }
}

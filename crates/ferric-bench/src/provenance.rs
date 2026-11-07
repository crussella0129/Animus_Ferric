//! Reproducibility digests, clocks and run identities used by benchmark
//! result records.

use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

pub fn sha256_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

/// Exactly 64 hexadecimal characters, in either case.
pub fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Command-line SHA-256 parser: exact length, normalized to lowercase.
pub fn parse_sha256(value: &str) -> Result<String, String> {
    if is_sha256_hex(value) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err("SHA-256 must contain exactly 64 hexadecimal characters".to_string())
    }
}

pub fn now_unix_ms() -> u64 {
    system_time_unix_ms(SystemTime::now()).unwrap_or_default()
}

pub fn system_time_unix_ms(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

/// A process-unique run identifier: `<prefix>-<start ms>-<pid>-<sequence>`.
pub fn new_run_id(prefix: &str, started_at_unix_ms: u64) -> String {
    static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);
    let sequence = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "{prefix}-{started_at_unix_ms}-{}-{sequence}",
        std::process::id()
    )
}

/// The commit the build environment supplied, if any.
pub fn git_commit() -> Option<String> {
    option_env!("FERRIC_GIT_COMMIT")
        .or(option_env!("VERGEN_GIT_SHA"))
        .or(option_env!("GITHUB_SHA"))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_known_vector_and_file_equally() {
        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(sha256_bytes(b"abc"), expected);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(sha256_file(&path).unwrap(), expected);
    }

    #[test]
    fn sha256_parser_is_exact_and_normalizes_case() {
        assert_eq!(parse_sha256(&"A".repeat(64)).unwrap(), "a".repeat(64));
        assert!(parse_sha256(&"a".repeat(63)).is_err());
        assert!(parse_sha256(&format!("{}z", "a".repeat(63))).is_err());
    }
}

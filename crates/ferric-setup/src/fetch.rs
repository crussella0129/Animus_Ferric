//! Verified, resumable downloads.
//!
//! Bytes stream into `<dest>.part` while a SHA-256 runs over them. The final
//! path appears only by renaming a part file whose digest matched, so a
//! half-downloaded or tampered file is never left where a later run would
//! load it. A part file left by an interrupted run is resumed with an HTTP
//! range request; its existing bytes are re-hashed first.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use crate::SetupError;

/// Accept `https`, or plain `http` only to a loopback host (a local mirror).
pub fn check_source_url(url: &str) -> Result<(), SetupError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| SetupError::Unsupported(format!("not a URL: {url}")))?;
    match parsed.scheme() {
        "https" => Ok(()),
        "http" if is_loopback(&parsed) => Ok(()),
        _ => Err(SetupError::Unsupported(format!(
            "refusing {url}: downloads use https (plain http only to a loopback mirror)"
        ))),
    }
}

fn is_loopback(url: &reqwest::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || bare
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// An HTTP client for setup: a Ferric user agent, redirects followed only to
/// `https` (or loopback `http`), and the environment's proxy settings.
pub fn client() -> Result<reqwest::Client, SetupError> {
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 10 {
            attempt.error("too many redirects")
        } else if check_source_url(attempt.url().as_str()).is_ok() {
            attempt.follow()
        } else {
            attempt.error("redirect to a non-https location")
        }
    });
    reqwest::Client::builder()
        .user_agent(concat!("ferric-setup/", env!("CARGO_PKG_VERSION")))
        .redirect(policy)
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| SetupError::Network(error.to_string()))
}

/// Progress callback: bytes so far, and the total when known.
pub type Progress<'a> = &'a mut (dyn FnMut(u64, Option<u64>) + Send);

/// Download `url` to `dest`, accepting it only if its SHA-256 is `sha256`.
/// An existing `dest` with the right digest is kept (nothing is fetched).
pub async fn download_verified(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    sha256: &str,
    expected_size: Option<u64>,
    what: &str,
    progress: Progress<'_>,
) -> Result<(), SetupError> {
    check_source_url(url)?;
    let expected = sha256.to_ascii_lowercase();
    if dest.is_file() && file_sha256(dest).await? == expected {
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| SetupError::io(parent, e))?;
    }
    let part = part_path(dest);

    // Resume: hash what an earlier run left, then ask for the rest.
    let mut hasher = Sha256::new();
    let mut have = 0u64;
    if let Ok(meta) = tokio::fs::metadata(&part).await {
        if expected_size.is_some_and(|size| meta.len() > size) {
            remove_quietly(&part).await;
        } else {
            have = hash_into(&part, &mut hasher).await?;
        }
    }
    let complete = expected_size.is_some_and(|size| have == size);
    if !complete {
        let mut request = client.get(url);
        if have > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
        }
        let mut response = request
            .send()
            .await
            .map_err(|e| SetupError::Network(format!("{what}: {e}")))?;
        let status = response.status();
        if have > 0 && status == reqwest::StatusCode::OK {
            // The server ignored the range: start over.
            hasher = Sha256::new();
            have = 0;
            remove_quietly(&part).await;
        } else if !(status.is_success()) {
            return Err(SetupError::Network(format!(
                "{what}: the server answered {status} for {url}"
            )));
        }
        let total = expected_size.or_else(|| response.content_length().map(|len| len + have));
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&part)
            .await
            .map_err(|e| SetupError::io(&part, e))?;
        progress(have, total);
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| SetupError::Network(format!("{what}: {e}")))?
        {
            hasher.update(&chunk);
            file.write_all(&chunk)
                .await
                .map_err(|e| SetupError::io(&part, e))?;
            have += chunk.len() as u64;
            if expected_size.is_some_and(|size| have > size) {
                drop(file);
                remove_quietly(&part).await;
                return Err(SetupError::Network(format!(
                    "{what}: the download is larger than the pinned size; discarded"
                )));
            }
            progress(have, total);
        }
        file.flush().await.map_err(|e| SetupError::io(&part, e))?;
        file.sync_all()
            .await
            .map_err(|e| SetupError::io(&part, e))?;
    }
    let actual = hex::encode(hasher.finalize());
    if actual != expected {
        remove_quietly(&part).await;
        return Err(SetupError::Checksum {
            what: what.to_string(),
            expected,
            actual,
        });
    }
    tokio::fs::rename(&part, dest)
        .await
        .map_err(|e| SetupError::io(dest, e))
}

pub fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    dest.with_file_name(name)
}

/// SHA-256 of a whole file, as lowercase hex.
pub async fn file_sha256(path: &Path) -> Result<String, SetupError> {
    let mut hasher = Sha256::new();
    hash_into(path, &mut hasher).await?;
    Ok(hex::encode(hasher.finalize()))
}

async fn hash_into(path: &Path, hasher: &mut Sha256) -> Result<u64, SetupError> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| SetupError::io(path, e))?;
    let mut buffer = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|e| SetupError::io(path, e))?;
        if read == 0 {
            return Ok(total);
        }
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
}

async fn remove_quietly(path: &Path) {
    let _ = tokio::fs::remove_file(path).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_or_loopback_http_is_accepted() {
        assert!(check_source_url("https://huggingface.co/x").is_ok());
        assert!(check_source_url("http://127.0.0.1:8080/x").is_ok());
        assert!(check_source_url("http://localhost:9/x").is_ok());
        assert!(check_source_url("http://[::1]:9/x").is_ok());
        assert!(check_source_url("http://huggingface.co/x").is_err());
        assert!(check_source_url("http://127.0.0.1.attacker.example/x").is_err());
        assert!(check_source_url("ftp://127.0.0.1/x").is_err());
        assert!(check_source_url("file:///etc/passwd").is_err());
    }

    #[test]
    fn the_part_file_sits_beside_the_destination() {
        assert_eq!(
            part_path(Path::new("/m/model.gguf")),
            PathBuf::from("/m/model.gguf.part")
        );
    }
}

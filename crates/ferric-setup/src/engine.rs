//! Installing the llama.cpp engine.
//!
//! Ferric picks the pinned release's CPU build for this OS and architecture,
//! reads that asset's SHA-256 from the GitHub release (GitHub records a
//! digest for every uploaded asset), downloads and verifies it, and extracts
//! it with the operating system's own `tar`. bsdtar ships with Windows 10 and
//! later, and reads `.zip` as well as `.tar.gz`, so no archive library is
//! needed. The extracted server must answer `--version` before it is
//! recorded as Ferric's engine.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::SetupError;
use crate::fetch::{Progress, check_source_url, download_verified};

pub const GITHUB_API: &str = "https://api.github.com";
pub const LLAMA_CPP_REPO: &str = "ggml-org/llama.cpp";
const RECORD: &str = "engine.json";

/// The release asset for a platform, or why there is none.
pub fn asset_name(tag: &str, os: &str, arch: &str) -> Result<String, SetupError> {
    let arch = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => {
            return Err(SetupError::Unsupported(format!(
                "no prebuilt llama.cpp engine for {os}/{other}; install llama-server on PATH instead"
            )));
        }
    };
    match os {
        "linux" => Ok(format!("llama-{tag}-bin-ubuntu-{arch}.tar.gz")),
        "macos" => Ok(format!("llama-{tag}-bin-macos-{arch}.tar.gz")),
        "windows" => Ok(format!("llama-{tag}-bin-win-cpu-{arch}.zip")),
        other => Err(SetupError::Unsupported(format!(
            "no prebuilt llama.cpp engine for {other}; install llama-server on PATH instead"
        ))),
    }
}

/// One asset from a GitHub release, with its verified-download facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

/// Find `name` in a GitHub release JSON. The asset must carry a
/// `sha256:` digest; an asset without one is refused rather than trusted.
pub fn find_asset(release: &Value, name: &str) -> Result<ReleaseAsset, SetupError> {
    let assets = release
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| SetupError::Engine("the release lists no assets".into()))?;
    let asset = assets
        .iter()
        .find(|asset| asset.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| SetupError::Engine(format!("the release has no asset named {name}")))?;
    let sha256 = asset
        .get("digest")
        .and_then(Value::as_str)
        .and_then(|digest| digest.strip_prefix("sha256:"))
        .filter(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| {
            SetupError::Engine(format!(
                "{name} has no SHA-256 digest in the release, so it cannot be verified; refusing it"
            ))
        })?
        .to_ascii_lowercase();
    let url = asset
        .get("browser_download_url")
        .and_then(Value::as_str)
        .ok_or_else(|| SetupError::Engine(format!("{name} has no download URL")))?
        .to_string();
    let size = asset.get("size").and_then(Value::as_u64).unwrap_or(0);
    Ok(ReleaseAsset {
        name: name.to_string(),
        url,
        sha256,
        size,
    })
}

/// Read the release for `tag` from a GitHub-compatible API base.
pub async fn fetch_release(
    client: &reqwest::Client,
    api_base: &str,
    tag: &str,
) -> Result<Value, SetupError> {
    let url = format!(
        "{}/repos/{LLAMA_CPP_REPO}/releases/tags/{tag}",
        api_base.trim_end_matches('/')
    );
    check_source_url(&url)?;
    let response = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| SetupError::Network(format!("llama.cpp release {tag}: {e}")))?;
    if !response.status().is_success() {
        return Err(SetupError::Network(format!(
            "llama.cpp release {tag}: GitHub answered {}",
            response.status()
        )));
    }
    response
        .json()
        .await
        .map_err(|e| SetupError::Network(format!("llama.cpp release {tag}: {e}")))
}

/// An installed engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledEngine {
    pub tag: String,
    pub server: PathBuf,
    pub version: String,
}

/// The server binary's file name on this platform.
pub fn server_file_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

/// Install the pinned engine under `root` (Ferric's engine directory).
pub async fn install(
    client: &reqwest::Client,
    api_base: &str,
    tag: &str,
    os: &str,
    arch: &str,
    root: &Path,
    progress: Progress<'_>,
) -> Result<InstalledEngine, SetupError> {
    let name = asset_name(tag, os, arch)?;
    let release = fetch_release(client, api_base, tag).await?;
    let asset = find_asset(&release, &name)?;
    let downloads = root.join("downloads");
    let archive = downloads.join(&asset.name);
    download_verified(
        client,
        &asset.url,
        &archive,
        &asset.sha256,
        (asset.size > 0).then_some(asset.size),
        &format!("llama.cpp {tag}"),
        progress,
    )
    .await?;
    let installed = extract_and_record(&archive, tag, root)?;
    let _ = std::fs::remove_file(&archive);
    Ok(installed)
}

/// Extract a verified archive into `root/llama.cpp-<tag>`, check the server
/// runs, and record it as Ferric's engine.
pub fn extract_and_record(
    archive: &Path,
    tag: &str,
    root: &Path,
) -> Result<InstalledEngine, SetupError> {
    let staging = root.join(format!(".staging-{tag}"));
    let target = root.join(format!("llama.cpp-{tag}"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| SetupError::io(&staging, e))?;
    let output = Command::new(tar_program())
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(&staging)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| SetupError::Engine(format!("cannot run tar: {e}")))?;
    if !output.status.success() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(SetupError::Engine(format!(
            "tar could not extract {}: {}",
            archive.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let relative = find_server(&staging, 3)
        .and_then(|found| found.strip_prefix(&staging).ok().map(Path::to_path_buf))
        .ok_or_else(|| {
            let _ = std::fs::remove_dir_all(&staging);
            SetupError::Engine(format!(
                "{} contains no {}",
                archive.display(),
                server_file_name()
            ))
        })?;
    let _ = std::fs::remove_dir_all(&target);
    std::fs::rename(&staging, &target).map_err(|e| SetupError::io(&target, e))?;
    let server = target.join(relative);
    let version = smoke_version(&server)?;
    let record = json!({
        "tag": tag,
        "server": server,
        "version": version,
    });
    let record_path = root.join(RECORD);
    std::fs::write(
        &record_path,
        serde_json::to_vec_pretty(&record).unwrap_or_default(),
    )
    .map_err(|e| SetupError::io(&record_path, e))?;
    Ok(InstalledEngine {
        tag: tag.to_string(),
        server,
        version,
    })
}

/// The engine Ferric installed earlier, if its server is still there.
pub fn managed_engine(root: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(root.join(RECORD)).ok()?;
    let record: Value = serde_json::from_str(&text).ok()?;
    let server = PathBuf::from(record.get("server")?.as_str()?);
    (server.is_absolute() && server.is_file()).then_some(server)
}

fn tar_program() -> PathBuf {
    if cfg!(windows) {
        // Never resolve tar through the current directory or PATH on
        // Windows: use the copy that ships in System32.
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        PathBuf::from(root).join("System32").join("tar.exe")
    } else {
        PathBuf::from("tar")
    }
}

fn find_server(dir: &Path, depth: usize) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let file_type = entry.file_type().ok()?;
        if file_type.is_file() && entry.file_name() == server_file_name() {
            return Some(path);
        }
        if file_type.is_dir() {
            subdirs.push(path);
        }
    }
    if depth == 0 {
        return None;
    }
    subdirs.sort();
    subdirs.iter().find_map(|sub| find_server(sub, depth - 1))
}

/// Run `server --version` with a deadline; the engine must answer.
fn smoke_version(server: &Path) -> Result<String, SetupError> {
    let mut child = Command::new(server)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| SetupError::Engine(format!("cannot run {}: {e}", server.display())))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| SetupError::Engine(e.to_string()))?
        {
            let output = child
                .wait_with_output()
                .map_err(|e| SetupError::Engine(e.to_string()))?;
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            if !status.success() {
                return Err(SetupError::Engine(format!(
                    "{} --version failed: {}",
                    server.display(),
                    text.trim()
                )));
            }
            let version = text
                .lines()
                .find(|line| line.contains("version"))
                .unwrap_or_else(|| text.lines().next().unwrap_or(""))
                .trim()
                .to_string();
            return Ok(version);
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(SetupError::Engine(format!(
                "{} --version did not finish within 30 s",
                server.display()
            )));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_supported_platform_maps_to_one_cpu_asset() {
        let tag = "b11780";
        assert_eq!(
            asset_name(tag, "linux", "x86_64").unwrap(),
            "llama-b11780-bin-ubuntu-x64.tar.gz"
        );
        assert_eq!(
            asset_name(tag, "linux", "aarch64").unwrap(),
            "llama-b11780-bin-ubuntu-arm64.tar.gz"
        );
        assert_eq!(
            asset_name(tag, "macos", "aarch64").unwrap(),
            "llama-b11780-bin-macos-arm64.tar.gz"
        );
        assert_eq!(
            asset_name(tag, "windows", "x86_64").unwrap(),
            "llama-b11780-bin-win-cpu-x64.zip"
        );
        assert!(asset_name(tag, "freebsd", "x86_64").is_err());
        assert!(asset_name(tag, "linux", "riscv64").is_err());
    }

    #[test]
    fn an_asset_needs_a_sha256_digest() {
        let release = json!({"assets": [
            {"name": "a.tar.gz", "size": 3, "browser_download_url": "https://x/a",
             "digest": "sha256:AB".to_string() + &"0".repeat(62)},
            {"name": "b.tar.gz", "size": 3, "browser_download_url": "https://x/b"},
            {"name": "c.tar.gz", "size": 3, "browser_download_url": "https://x/c", "digest": "md5:00"},
        ]});
        let a = find_asset(&release, "a.tar.gz").unwrap();
        assert_eq!(a.sha256, format!("ab{}", "0".repeat(62)));
        assert_eq!(a.url, "https://x/a");
        assert!(find_asset(&release, "b.tar.gz").is_err());
        assert!(find_asset(&release, "c.tar.gz").is_err());
        assert!(find_asset(&release, "missing.tar.gz").is_err());
    }

    #[test]
    fn no_record_means_no_managed_engine() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(managed_engine(dir.path()), None);
        std::fs::write(
            dir.path().join(RECORD),
            r#"{"server":"/nonexistent/llama-server"}"#,
        )
        .unwrap();
        assert_eq!(managed_engine(dir.path()), None);
    }
}

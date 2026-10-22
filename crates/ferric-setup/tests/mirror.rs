//! Sprint 128 (INT-0016): verified downloads and engine installs against a
//! local mirror. No network: the mirror is an axum server on loopback that
//! speaks just enough of the GitHub release API and HTTP ranges.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use ferric_setup::SetupError;
use ferric_setup::fetch::{client, download_verified, part_path};
use sha2::{Digest, Sha256};

#[derive(Clone, Default)]
struct Mirror {
    bytes: Arc<Vec<u8>>,
    release: Arc<serde_json::Value>,
    ranges: Arc<Mutex<Vec<String>>>,
}

async fn serve_bytes(State(mirror): State<Mirror>, headers: HeaderMap) -> Response {
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    if let Some(range) = range {
        mirror.ranges.lock().unwrap().push(range.clone());
        let start: usize = range
            .trim_start_matches("bytes=")
            .trim_end_matches('-')
            .parse()
            .unwrap();
        return (
            StatusCode::PARTIAL_CONTENT,
            Body::from(mirror.bytes[start..].to_vec()),
        )
            .into_response();
    }
    Body::from(mirror.bytes.to_vec()).into_response()
}

async fn serve_release(State(mirror): State<Mirror>) -> Response {
    axum::Json((*mirror.release).clone()).into_response()
}

async fn start(mirror: Mirror) -> String {
    let app = Router::new()
        .route("/file", get(serve_bytes))
        .route(
            "/repos/ggml-org/llama.cpp/releases/tags/{tag}",
            get(serve_release),
        )
        .with_state(mirror);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn payload() -> Vec<u8> {
    (0..300_000u32).flat_map(|i| i.to_le_bytes()).collect()
}

#[tokio::test]
async fn a_matching_download_lands_and_leaves_no_part_file() {
    let bytes = payload();
    let mirror = Mirror {
        bytes: Arc::new(bytes.clone()),
        ..Mirror::default()
    };
    let base = start(mirror).await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("models").join("m.gguf");
    let mut seen = 0;
    let mut progress = |done: u64, _total: Option<u64>| seen = done;
    download_verified(
        &client().unwrap(),
        &format!("{base}/file"),
        &dest,
        &sha(&bytes),
        Some(bytes.len() as u64),
        "test model",
        &mut progress,
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    assert!(!part_path(&dest).exists());
    assert_eq!(seen, bytes.len() as u64);
}

#[tokio::test]
async fn a_mismatched_download_is_discarded() {
    let bytes = payload();
    let mirror = Mirror {
        bytes: Arc::new(bytes.clone()),
        ..Mirror::default()
    };
    let base = start(mirror).await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("m.gguf");
    let error = download_verified(
        &client().unwrap(),
        &format!("{base}/file"),
        &dest,
        &"0".repeat(64),
        Some(bytes.len() as u64),
        "test model",
        &mut |_, _| {},
    )
    .await
    .unwrap_err();
    assert!(matches!(error, SetupError::Checksum { .. }), "{error}");
    assert!(
        !dest.exists(),
        "a failed download must never reach its final path"
    );
    assert!(!part_path(&dest).exists());
}

#[tokio::test]
async fn an_interrupted_download_resumes_with_a_range_request() {
    let bytes = payload();
    let mirror = Mirror {
        bytes: Arc::new(bytes.clone()),
        ..Mirror::default()
    };
    let ranges = mirror.ranges.clone();
    let base = start(mirror).await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("m.gguf");
    let half = bytes.len() / 2;
    std::fs::write(part_path(&dest), &bytes[..half]).unwrap();
    download_verified(
        &client().unwrap(),
        &format!("{base}/file"),
        &dest,
        &sha(&bytes),
        Some(bytes.len() as u64),
        "test model",
        &mut |_, _| {},
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    assert_eq!(*ranges.lock().unwrap(), vec![format!("bytes={half}-")]);
}

#[tokio::test]
async fn an_existing_verified_file_is_not_downloaded_again() {
    let bytes = payload();
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("m.gguf");
    std::fs::write(&dest, &bytes).unwrap();
    // No server at all: a fetch would fail.
    download_verified(
        &client().unwrap(),
        "http://127.0.0.1:9/never",
        &dest,
        &sha(&bytes),
        Some(bytes.len() as u64),
        "test model",
        &mut |_, _| {},
    )
    .await
    .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn the_engine_installs_from_a_verified_release_and_is_recorded() {
    use std::os::unix::fs::PermissionsExt as _;
    let tag = "b0";
    let work = tempfile::tempdir().unwrap();
    // Package a stand-in server the way llama.cpp's release does:
    // llama-<tag>/llama-server inside a .tar.gz.
    let inner = work.path().join(format!("llama-{tag}"));
    std::fs::create_dir_all(&inner).unwrap();
    let server = inner.join("llama-server");
    std::fs::write(&server, "#!/bin/sh\necho \"version: 0 (stand-in)\"\n").unwrap();
    std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o755)).unwrap();
    let name = ferric_setup::engine::asset_name(tag, std::env::consts::OS, std::env::consts::ARCH)
        .unwrap();
    let archive = work.path().join(&name);
    let status = std::process::Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(work.path())
        .arg(format!("llama-{tag}"))
        .status()
        .unwrap();
    assert!(status.success());
    let bytes = std::fs::read(&archive).unwrap();

    let mut mirror = Mirror {
        bytes: Arc::new(bytes.clone()),
        ..Mirror::default()
    };
    let base = start(mirror.clone()).await;
    mirror.release = Arc::new(serde_json::json!({"assets": [{
        "name": name,
        "size": bytes.len(),
        "digest": format!("sha256:{}", sha(&bytes)),
        "browser_download_url": format!("{base}/file"),
    }]}));
    // The router captured the old state; restart with the release filled in.
    let base = start(mirror).await;

    let root = work.path().join("engine");
    let installed = ferric_setup::engine::install(
        &client().unwrap(),
        &base,
        tag,
        std::env::consts::OS,
        std::env::consts::ARCH,
        &root,
        &mut |_, _| {},
    )
    .await
    .unwrap();
    assert!(
        installed.version.contains("version: 0"),
        "{}",
        installed.version
    );
    assert_eq!(
        ferric_setup::engine::managed_engine(&root),
        Some(installed.server.clone())
    );
    assert!(
        installed
            .server
            .starts_with(root.join(format!("llama.cpp-{tag}")))
    );
    assert!(
        !root.join("downloads").join(&name).exists(),
        "the archive is cleaned up"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn an_engine_asset_with_the_wrong_digest_is_refused() {
    let tag = "b0";
    let name = ferric_setup::engine::asset_name(tag, std::env::consts::OS, std::env::consts::ARCH)
        .unwrap();
    let bytes = payload();
    let mut mirror = Mirror {
        bytes: Arc::new(bytes.clone()),
        ..Mirror::default()
    };
    let base = start(mirror.clone()).await;
    mirror.release = Arc::new(serde_json::json!({"assets": [{
        "name": name,
        "size": bytes.len(),
        "digest": format!("sha256:{}", "0".repeat(64)),
        "browser_download_url": format!("{base}/file"),
    }]}));
    let base = start(mirror).await;
    let work = tempfile::tempdir().unwrap();
    let root = work.path().join("engine");
    let error = ferric_setup::engine::install(
        &client().unwrap(),
        &base,
        tag,
        std::env::consts::OS,
        std::env::consts::ARCH,
        &root,
        &mut |_, _| {},
    )
    .await
    .unwrap_err();
    assert!(matches!(error, SetupError::Checksum { .. }), "{error}");
    assert_eq!(ferric_setup::engine::managed_engine(&root), None);
}

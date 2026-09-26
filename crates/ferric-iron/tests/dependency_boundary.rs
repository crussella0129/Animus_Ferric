//! INT-0011 AC-1: the constrained-decoding core's dependency boundary, pinned.
//!
//! Another harness (the Hermes valve) depends on this crate precisely because
//! it pulls in nothing else: no async runtime, no HTTP client, no filesystem
//! layer and none of Ferric's harness crates. A future edit that adds such a
//! dependency — to `ferric-iron` directly, or to `ferric-core`, which it builds
//! on — must fail here, naming the crate, rather than silently widening what
//! every consumer compiles. Dev-dependencies do not ship and are not counted.

use std::path::Path;

/// Normal (shipped) dependencies `ferric-iron` may declare.
const IRON_ALLOWED: &[&str] = &["ferric-core", "serde", "serde_json", "thiserror"];
/// Normal dependencies of `ferric-core`, which `ferric-iron` inherits.
const CORE_ALLOWED: &[&str] = &["serde", "serde_json", "thiserror"];

/// Every normal dependency a manifest declares: `[dependencies]` plus any
/// `[target.'cfg(..)'.dependencies]` table, which also ships.
fn shipped_dependencies(manifest: &str) -> Result<Vec<String>, String> {
    let table: toml::Table = toml::from_str(manifest).map_err(|error| error.to_string())?;
    let mut names: Vec<String> = table
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .map(|deps| deps.keys().cloned().collect())
        .unwrap_or_default();
    if let Some(targets) = table.get("target").and_then(toml::Value::as_table) {
        for target in targets.values() {
            if let Some(deps) = target.get("dependencies").and_then(toml::Value::as_table) {
                names.extend(deps.keys().cloned());
            }
        }
    }
    Ok(names)
}

/// `Ok` when every shipped dependency is allowed; otherwise an error naming
/// the manifest and each offending crate.
fn check_boundary(manifest_name: &str, manifest: &str, allowed: &[&str]) -> Result<(), String> {
    let offending: Vec<String> = shipped_dependencies(manifest)?
        .into_iter()
        .filter(|name| !allowed.contains(&name.as_str()))
        .collect();
    if offending.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{manifest_name} ships dependencies outside the constrained-decoding core boundary: {offending:?} (allowed: {allowed:?})"
        ))
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

#[test]
fn iron_dependencies_are_within_boundary() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let iron = crate_dir.join("Cargo.toml");
    let core = crate_dir.join("../ferric-core/Cargo.toml");
    check_boundary("crates/ferric-iron/Cargo.toml", &read(&iron), IRON_ALLOWED).unwrap();
    check_boundary("crates/ferric-core/Cargo.toml", &read(&core), CORE_ALLOWED).unwrap();
}

#[test]
fn boundary_check_names_offending_crate() {
    let manifest = "[package]\nname = \"x\"\n\n[dependencies]\nserde = \"1\"\ntokio = \"1\"\n\n[target.'cfg(windows)'.dependencies]\nwindows-sys = \"0.61\"\n\n[dev-dependencies]\ntempfile = \"3\"\n";
    let error = check_boundary("synthetic/Cargo.toml", manifest, IRON_ALLOWED).unwrap_err();
    assert!(error.contains("synthetic/Cargo.toml"), "{error}");
    assert!(error.contains("tokio"), "{error}");
    assert!(error.contains("windows-sys"), "{error}");
    assert!(
        !error.contains("tempfile"),
        "dev-dependencies must not count: {error}"
    );
}

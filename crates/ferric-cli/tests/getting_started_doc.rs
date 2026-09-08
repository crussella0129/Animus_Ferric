//! Sprint 125, T-12503: the documented human entry point must describe the
//! install-once / run-in-your-project-folder / configured-models-dir flow.
//!
//! This is the executed check behind the plan's `getting_started_describes_
//! installed_flow`: the EARS clause is "WHEN `welcome()` / getting-started is
//! read, THEN it SHALL describe the installed-binary, run-from-your-project,
//! configured-models-dir flow (no 'run cargo r in the repo')." `welcome()`'s
//! strings are covered by the CLI's own tests; this pins the prose doc so it
//! cannot silently drift back to the old "run cargo r in the repo" guidance.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is <root>/crates/ferric-cli.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate sits two levels below the workspace root")
        .to_path_buf()
}

#[test]
fn getting_started_describes_installed_flow() {
    let doc = repo_root().join("docs").join("getting-started.md");
    let text = std::fs::read_to_string(&doc)
        .unwrap_or_else(|error| panic!("read {}: {error}", doc.display()));

    // The install-once binary path, the configured models directory, and running
    // in your own project folder must all be present.
    for needle in [
        "cargo install --path crates/ferric-cli",
        "FERRIC_MODELS_DIR",
        "just run `ferric`",
    ] {
        assert!(
            text.contains(needle),
            "getting-started.md must describe the installed run-from-anywhere flow; missing: {needle:?}"
        );
    }

    // The superseded instruction must be gone: the whole point of INT-0008's
    // human entry point is that you do not `cargo run` inside the repo.
    assert!(
        !text.contains("Run cargo r in a terminal"),
        "getting-started.md still tells the user to run cargo r in the repo"
    );
}

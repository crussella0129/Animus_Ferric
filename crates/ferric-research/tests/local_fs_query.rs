//! `LocalFsRetriever` query matching (ADR-078).
//!
//! The query used to be matched as ONE literal lowercase substring, so any
//! multi-word research query — the natural way to ask for research — found
//! nothing unless that exact phrase appeared verbatim in a file. Measured live
//! in sprint 87: `ferric query --research "project notes configuration"` over a
//! workspace plainly containing those words injected no research context at all,
//! and said nothing about it.

use ferric_research::{LocalFsRetriever, Retriever};

fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("research_notes.md"),
        "# Project notes\nThe configuration file lives at the repository root.\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("unrelated.md"),
        "# Shopping\nMilk, bread, coffee.\n",
    )
    .unwrap();
    dir
}

fn retrieve(dir: &tempfile::TempDir, query: &str) -> Vec<String> {
    let r = LocalFsRetriever::with_caps(dir.path().to_path_buf(), 50, 1024 * 1024);
    futures_executor::block_on(r.retrieve(query))
        .expect("retrieve")
        .into_iter()
        .map(|c| c.source)
        .collect()
}

#[test]
fn a_single_word_query_matches() {
    let dir = workspace();
    assert_eq!(retrieve(&dir, "configuration"), vec!["research_notes.md"]);
}

/// The regression: this returned nothing.
#[test]
fn a_multi_word_query_matches() {
    let dir = workspace();
    assert_eq!(
        retrieve(&dir, "project notes configuration"),
        vec!["research_notes.md"],
        "each word is a term; the query is not one literal phrase"
    );
}

/// Terms are ANDed, not ORed. OR would return most of the tree — and every
/// chunk costs one quarantine inference, so a loose match is expensive as well
/// as useless.
#[test]
fn terms_are_conjunctive() {
    let dir = workspace();
    // "coffee" appears only in unrelated.md, "configuration" only in notes.
    assert!(
        retrieve(&dir, "configuration coffee").is_empty(),
        "no single file contains both terms"
    );
    // Each alone still matches its own file.
    assert_eq!(retrieve(&dir, "coffee"), vec!["unrelated.md"]);
}

/// Word order and punctuation must not change the result.
#[test]
fn order_and_punctuation_do_not_matter() {
    let dir = workspace();
    let a = retrieve(&dir, "configuration project");
    let b = retrieve(&dir, "  project,  configuration!  ");
    assert_eq!(a, b);
    assert_eq!(a, vec!["research_notes.md"]);
}

/// A blank query must not sweep the whole workspace into the quarantine.
#[test]
fn a_blank_query_matches_nothing() {
    let dir = workspace();
    assert!(retrieve(&dir, "   ").is_empty());
}

/// Filename matches still count, not just content.
#[test]
fn a_filename_term_still_matches() {
    let dir = workspace();
    assert_eq!(retrieve(&dir, "research notes"), vec!["research_notes.md"]);
}

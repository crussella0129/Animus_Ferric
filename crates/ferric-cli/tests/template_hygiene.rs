//! Ferric is meant to be usable as a template, which means the tree must not
//! carry the identity of whatever machine happened to grow it (sprint 105,
//! ADR-096).
//!
//! Sprint 105 removed the machine identity that had accumulated in test
//! fixtures, script defaults and docs. That was a one-time cleanup, and a
//! one-time cleanup decays: the natural way to write a parser test is to paste
//! real output from the machine in front of you, which is exactly how every
//! instance of this got here. This test is the ratchet.
//!
//! # What it matches, and what it deliberately does not
//! Only **shapes that can only be identity** — never topics. This repo's prose
//! is full of the words "tailscale" and "NAS"; none of that is a leak. A real
//! MagicDNS suffix, a concrete home directory, and a private LAN address are.
//!
//! Documentation-range values are the escape hatch and stay legal:
//! `tailnet-example.ts.net`, `100.64.0.x` (inside Tailscale's real CGNAT range,
//! so fixtures stay representative), `example-host`, `user@`.

use std::path::{Path, PathBuf};

/// Directories that carry the project's *history* rather than its template
/// surface. `decisions.md` and `agent-tasks/` record what was measured and on
/// what — rewriting them to remove the machine would falsify the evidence they
/// cite, so they are excluded by decision, not by oversight (ADR-096).
const EXCLUDED: &[&str] = &[
    "target",
    ".git",
    "sprints",
    "benchmarks",
    "decisions.md",
    "agent-tasks",
    // This file states the forbidden shapes in order to forbid them.
    "template_hygiene.rs",
];

/// Each rule is (human-readable name, matcher). Hand-written matchers rather
/// than a regex dependency — the crate has no regex dep and this needs three
/// fixed shapes, not a language.
type Rule = (&'static str, fn(&str) -> bool);

const RULES: &[Rule] = &[
    (
        "a real MagicDNS tailnet suffix (tail<digits>.ts.net)",
        has_magicdns_suffix,
    ),
    (
        "a concrete home directory (X:\\Users\\<name> or /home/<name>)",
        has_concrete_home,
    ),
    ("a private LAN address (192.168.x.y)", has_private_lan_ip),
];

/// `tail` + at least four digits + `.ts.net` — a tailnet's unique id. The
/// example suffix (`tailnet-example.ts.net`) has no digit run and is fine.
fn has_magicdns_suffix(line: &str) -> bool {
    line.match_indices("tail").any(|(i, _)| {
        let rest = &line[i + 4..];
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        digits >= 4 && rest[digits..].starts_with(".ts.net")
    })
}

/// `X:\Users\name\` / `X:/Users/name/`, or `/home/name/`. A placeholder in
/// angle brackets (`C:\Users\<you>\`) is not a concrete path and passes.
///
/// Backslashes are collapsed first, because the most likely place for a leaked
/// path is **inside a Rust string literal**, where `C:\Users\alice` is written
/// `"C:\\Users\\alice"` and reaches this function doubled. Matching only the
/// single-backslash form would have missed exactly the case this guard exists
/// for — caught by the self-test below rather than by review.
fn has_concrete_home(line: &str) -> bool {
    let line = &line.replace("\\\\", "\\");
    let named_segment_after = |hay: &str, marker: &str| -> bool {
        hay.match_indices(marker).any(|(i, _)| {
            let rest = &hay[i + marker.len()..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_' || *c == '-')
                .collect();
            // A generic stand-in is the point of the escape hatch.
            !name.is_empty() && !matches!(name.as_str(), "you" | "user" | "username" | "x" | "me")
        })
    };
    let lower = line.to_ascii_lowercase();
    named_segment_after(&lower, "\\users\\")
        || named_segment_after(&lower, ":/users/")
        || named_segment_after(&lower, "/home/")
}

/// A home/office LAN address. Nothing about a template should know one.
fn has_private_lan_ip(line: &str) -> bool {
    line.match_indices("192.168.").any(|(i, _)| {
        line[i + 8..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
    })
}

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is <root>/crates/ferric-cli.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate sits two levels below the workspace root")
        .to_path_buf()
}

fn scan(dir: &Path, findings: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if EXCLUDED.contains(&name.as_str()) || name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            scan(&path, findings);
            continue;
        }
        let interesting = matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rs" | "md" | "toml" | "json" | "yml" | "yaml" | "ps1" | "sh" | "py")
        );
        if !interesting {
            continue;
        }
        // Non-UTF8 files are not text we can leak identity through.
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            for (rule, matches) in RULES {
                if matches(line) {
                    findings.push(format!("{}:{}: {}", path.display(), n + 1, rule));
                }
            }
        }
    }
}

#[test]
fn tracked_sources_carry_no_machine_identity() {
    let root = repo_root();
    let mut findings = Vec::new();
    for sub in ["crates", "docs", "tools", "docker"] {
        scan(&root.join(sub), &mut findings);
    }
    scan_file(&root.join("README.md"), &mut findings);

    assert!(
        findings.is_empty(),
        "machine identity found in tracked sources — use documentation values \
         (tailnet-example.ts.net, 100.64.0.x, example-host, C:\\Users\\<you>):\n{}",
        findings.join("\n")
    );
}

fn scan_file(path: &Path, findings: &mut Vec<String>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    for (n, line) in text.lines().enumerate() {
        for (rule, matches) in RULES {
            if matches(line) {
                findings.push(format!("{}:{}: {}", path.display(), n + 1, rule));
            }
        }
    }
}

/// The guard has to be shown rejecting something, or it is only *assumed* to
/// work — the lesson from sprint 96's skip-and-pass and sprint 101's false
/// positive. Each rule is exercised against a line that must trip it and one
/// that must not.
#[test]
fn each_rule_rejects_identity_and_accepts_the_documentation_value() {
    assert!(has_magicdns_suffix("DNSName: box.tail944782.ts.net."));
    assert!(!has_magicdns_suffix(
        "DNSName: example-host.tailnet-example.ts.net."
    ));

    // A plain path as it appears in prose or a script.
    assert!(has_concrete_home("path = C:\\Users\\alice\\proj"));
    assert!(has_concrete_home("/home/alice/.config/ferric"));
    // The same path as it appears ON DISK inside a Rust string literal, with
    // its backslashes doubled — the form the guard originally missed.
    assert!(has_concrete_home(r#"let p = "C:\\Users\\alice\\proj";"#));
    assert!(!has_concrete_home("C:\\Users\\<you>\\proj"));
    assert!(!has_concrete_home("/home/x/.config/ferric"));

    assert!(has_private_lan_ip("NAS at 192.168.86.27"));
    assert!(!has_private_lan_ip("tailnet peer at 100.64.0.2"));
}

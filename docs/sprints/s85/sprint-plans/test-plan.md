# Finalized - DO NOT EDIT

# Sprint 85 — Test Plan

## Baseline (must be green before any finding is trusted)

Cold `cargo build`, `cargo test --workspace`, `cargo clippy --all-targets`,
`cargo fmt --check`, and Dark Matter's `verify-spec.sh`.

## Probes (must FAIL on current `main` to establish a defect)

| Probe | Asserts | Expected |
|---|---|---|
| approver prompt count | one tool call asks the human at most once | FAIL |
| taint false positives | benign restatements of a digest are not blocked | FAIL |
| taint true positive | the injected sentence IS still caught | PASS (control) |

The true-positive control matters: without it, "the taint set blocks things" is
indistinguishable from "the taint set blocks everything".

## Class sweeps (expected clean)

Production `unwrap`/`expect` outside `#[cfg(test)]`; `not wired`/TODO/FIXME
comments; backlog entries citing files that no longer exist.

## Exit

All probes deleted, `cargo test --workspace` green, `git status` showing only the
intended documents.

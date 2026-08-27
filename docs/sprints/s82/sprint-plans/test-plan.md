# Finalized - DO NOT EDIT

# Sprint 82 — Test Plan

The sprint's subject *is* the test suite, so "testing" here means two things:
running the existing suite, and writing throwaway tests that fail on `main` to
convert inspection-derived claims into demonstrated ones.

## 1. Baseline (the s81-blocked checks)

- `cargo build --workspace --all-targets` cold — expect clean.
- `cargo test --workspace` — record pass/fail and suite count.
- `cargo clippy --workspace --all-targets` — expect 0 warnings.
- `cargo fmt --all --check` — expect clean.
- Dark Matter `scripts/verify-spec.sh` — expect PASS 61 / FAIL 0.

## 2. Defect probes (must FAIL on current `main` to prove the defect)

| Probe | Asserts | Expected |
|---|---|---|
| `a1_probe` | longest user message ≤ 4,200 chars after a large `read_file` | FAIL |
| `a3_probe` | staged set unchanged across one `Vcs::snapshot` | FAIL |
| `a6_probe` | `fetch_reference(query="Go")` matches an all-Go vault | FAIL |
| `dm_probe` | DM-legal `{"target":…}` call accepted; return parses as DM's JSON envelope | FAIL ×2 |

A probe that *passes* refutes the finding — that is the point of writing them.

## 3. Vestigial proof

Remove the 6 claimed-unused deps, move `tokio` to `ferric-vcs` dev-deps, run
`cargo check --workspace --all-targets`. Exit 0 proves B3. Revert afterwards.

## 4. Exit criteria

All probes run and recorded; all Cargo experiments reverted; `git status` clean
apart from the three intended documents.

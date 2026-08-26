# Sprint 82 — Test Report

## 1. Baseline suite — GREEN

| Check | Result | Exit |
|---|---|---|
| `cargo build --workspace --all-targets` (cold) | clean, **0 warnings**, 31.47s | 0 |
| `cargo test --workspace` | **463 passed / 0 failed / 2 ignored**, 52 suites | 0 |
| `cargo clippy --workspace --all-targets` | **0 warnings, 0 errors** | 0 |
| `cargo fmt --all --check` | clean | 0 |
| DM `scripts/verify-spec.sh` | **PASS 61 / FAIL 0** | 0 |

484 `#[test]` attributes exist; 463 execute, 2 are `#[ignore]`d, the rest are
`cfg`-gated. All three checks sprint 81 was blocked on now run.

## 2. Defect probes — all failed as predicted (the defects are real)

### a1_probe — tool-output truncation
```
SPRINT82_PROBE longest_user_message_chars=20028
panicked: ADR-002 says the model sees the TRUNCATED view (4,000 chars),
          but the longest user message carries 20028 chars
```
**FAIL → A1 PROVEN.** 5× context-budget overrun on a single `read_file`.

### a3_probe — git index preservation
```
SPRINT82_PROBE staged_before="staged.txt" staged_after=""
panicked: snapshot() must leave the user's staging area untouched
```
**FAIL → A3 PROVEN.** Silent data loss, once per turn.

Follow-up experiment (scratch repo, not a cargo test) — s81's proposed fix:
```
BEFORE staged: 'staged.txt '   AFTER 'git read-tree HEAD' staged: ''
```
**s81's fix is also wrong.** The verified fix, temporary `GIT_INDEX_FILE`:
```
BEFORE staged: 'staged.txt '   AFTER temp-index snapshot staged: 'staged.txt '
tree contents: base.txt staged.txt untracked.txt
```
Index preserved *and* untracked files still captured.

### a6_probe — short-token queries
```
SPRINT82_PROBE fetch_reference(query="Go")
  -> "No reference chunk matched query \"Go\" (searched 1 chunk(s)…)"
```
**FAIL → A6 PROVEN.** The tool saw the chunk and rejected it.

### dm_probe — Dark Matter contract (2 tests)
```
SPRINT82_PROBE dm_legal_call -> Err("missing required string argument: query")
SPRINT82_PROBE return_payload -> "### ref://runtime.md#0\n# tokio spawn\n…"
```
**FAIL ×2 → divergence PROVEN.** A DM-schema-legal call is hard-rejected, and the
return is markdown where DM specifies `{chunks:[{uri,text,score}], truncated}`.

## 3. Vestigial proof — B3

Removed `thiserror` (ferric-bench/-tools/-trace), `ferric-core` (ferric-guard),
`ferric-guard` (ferric-research); moved `tokio` → `[dev-dependencies]` in
ferric-vcs.

```
cargo check --workspace --all-targets  →  CHECK_EXIT=0
```

**B3 PROVEN.** All six are genuinely unused. Reverted after measuring.

## 4. Why the green suite misses A1–A4

Not luck — one structural shape. Each defect is covered *up to* a crate boundary
and not *across* it:

- `registry.rs:423` tests that the Registry **computes** the truncated view, and
  passes, because that end is correct. Nothing tests that the loop **uses** it.
- `truncation_tests.rs` reads like A1 coverage and is not — it tests cut-off
  *model completions*, an unrelated mechanism that shares a word.
- `background_tasks.rs` covers `manage_task`'s happy path; every panic path, the
  `send_input` race, and all concurrent access are untested.

The suite is honest about what it covers. The gap is that no test follows a value
across the seam where it is dropped.

## 5. Final state

All probes deleted, all Cargo experiments reverted. `git status` shows only the
three intended documents (`docs/verification-2026-07.md`, `decisions.md`,
`README.md`). Verified: `cargo check --workspace --all-targets` exit 0 on the
restored tree.

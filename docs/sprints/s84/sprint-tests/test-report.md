# Sprint 84 — Test Report

## Gate

| Check | Result |
|---|---|
| `cargo test --workspace` | **503 passed / 0 failed** (487 at sprint start, +16) |
| `cargo clippy --workspace --all-targets` | 0 warnings |
| `cargo fmt --all --check` | clean |
| DM `scripts/verify-spec.sh` | **PASS 62 / FAIL 0 / SKIP 0** |

## New coverage

| Area | Tests |
|---|---|
| A4 / blocking | 4 unit (poison recovery, status labels, no-runtime, current-thread, multi-thread) |
| A4 / background tasks | 5 integration (send_input errors off current-thread + succeeds on multi-thread + pipe survives a second write; unknown task errors on every action; finished tasks removable) |
| Task-id collision | 1 integration (back-to-back tasks get distinct ids, all present in the registry) |
| A7 | 4 unit (approve runs it; reject denies and the handler never runs; no approver denies; untainted never asks) |
| A5 | 6 unit (default denies network; default requires gVisor; caps always dropped; proxy routes; unrestricted must be named; command follows image) |
| Dark Matter | 6 unit (DM-shaped call accepted; target scopes; target matches folder prefix; capped says so; uncapped doesn't; schema declares the cross-repo argument set) |

## Two defects found by testing something adjacent

1. **`shell_exec` had A4's panic pair too.** Found while writing the
   `manage_task` current-thread test — and it matters more, being Ring-0.
2. **Background-task ids collided** (`task-{millis}`). Surfaced as two tests
   flaking against each other. I first attributed it to shared global state and
   moved on; that attribution was wrong and the flake was the bug.

## A check that had to be tested before it could be trusted

DM's new `fetch_reference` schema check failed a negative control twice:

- grepping `"required": ["query"]` **false-positives** on the legitimate `anyOf`
  branch — it reported divergence against correct code;
- grepping the whole file **false-negatives** — a test mentioning
  `input_schema["anyOf"]` satisfied it even with the real declaration disabled.

Now scoped to the text above `#[cfg(test)]`, and confirmed both ways: green
against the current tree, red when the declaration is broken.

## Left open deliberately

DM SPEC §6.2's `{chunks:[{uri,text,score}], truncated}` return envelope vs
Ferric's markdown. Changing it alters what every small model sees and would
invalidate ADR-071's measured 97.5% prompt reduction — a decision for a
measurement, not a refactor.

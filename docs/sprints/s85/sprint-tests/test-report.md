# Sprint 85 — Test Report

## Baseline — clean-room, green

| Check | Result |
|---|---|
| `cargo build --workspace --all-targets` (cold, post-`cargo clean`) | clean, **0 warnings**, 41s |
| `cargo test --workspace` | **503 passed / 0 failed / 2 ignored**, 53 suites |
| `cargo clippy --workspace --all-targets` | 0 warnings |
| `cargo fmt --all --check` | clean |
| DM `scripts/verify-spec.sh` | **PASS 62 / FAIL 0 / SKIP 0** |

## Probes — 2 failed as predicted, 1 control passed

### E1 — approver prompt count
```
SPRINT85_PROBE approver_prompt_count=2 stop=TaskComplete
panicked: one tool call must ask the human at most once; asked 2 times
```
**FAIL → E1 PROVEN.**

### E2 — taint false positives
```
SPRINT85_PROBE benign_writes_blocked=3/3
  BLOCKED: The configuration file lives at the repository root.
  BLOCKED: Tests are run with cargo test across the workspace.
  BLOCKED: The project is a Rust workspace with several crates.
```
**FAIL → E2 PROVEN.** The paired true-positive control **passed** — the injected
sentence is still caught — which is what makes this a precision problem rather
than "the taint set is broken". Both halves were needed to say anything useful.

### E3 — run/replay cap
Attempted as an integration probe; abandoned deliberately. `replay()` refuses a
session that reached `SessionEnd`, and `TraceProjector` is not exported from the
crate root, so the divergence cannot be reached from an integration test. That
non-export is itself corroboration. Recorded **CONFIRMED by inspection** with
exact line cites (`run.rs:560` vs `replay.rs:63`, `compact.rs:224`) rather than
inflated to PROVEN.

## Class sweeps — clean

| Sweep | Result |
|---|---|
| Production `unwrap`/`expect` in tools/loop/guard | **6 sites, all safe idioms** — constant regex (`grammar.rs:25`), capture groups guaranteed by a successful match (`:29-30`), `take()` after `Stdio::piped()` (`shell_exec.rs:144-145`) |
| `not wired` / `not implemented` / TODO / FIXME | none outstanding; all hits describe fixes already made |
| Backlog entries citing dead files | none — all seven pre-audit items cite live files |

## Final state

All probes deleted; `cargo test --workspace` exit 0 afterwards; `git status`
shows only `docs/verification-2026-07-round2.md`, `decisions.md`, `README.md`
and `agent-tasks/agent-tasks.md`.

## What this round did not test

Everything in §4 of the report: no live model since ~sprint 26, the sandbox never
run against Docker, and both tuning constants measured only against synthetic
inputs. The green suite above is evidence about the mocks as much as the code.

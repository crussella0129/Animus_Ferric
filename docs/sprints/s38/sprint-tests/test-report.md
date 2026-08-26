# Sprint 38 Test Report — Persistent configuration + `Animus.md` (ADR-048)

## Summary
- `ferric-cli` unit tests (default features): 64 passed / 0 failed / 64 total (up from 41
  pre-sprint) — incl. `config.rs`'s layered-load/path-resolution suite, the new
  `merge_backend_opts` suite (test-critic C-001/C-003), and `Animus.md`'s pure `fold_animus_md`
  tests.
- `ferric-cli` integration tests (`tests/cli.rs`, real subprocess): 21 passed / 0 failed / 21 total
  (up from 12 pre-sprint) — config precedence for `params_b`/`max_ring`/`stream` (test-critic
  C-002), the `model_key` regression proof (plan-critic C-001), the malformed-config `Note`
  regression, and `Animus.md`'s fold + presence-`Note` proofs.
- `ferric-cli` under `--features backend-openai` / `--features backend-mistralrs`: unaffected,
  both clean.
- Full workspace (`cargo test --workspace`): **all crates green**, no regressions in any
  previously-shipped sprint's tests.
- Lint/format: `cargo clippy -p ferric-cli --all-targets -- -D warnings` clean on default,
  `backend-openai`, and `backend-mistralrs` feature sets; `cargo fmt --all --check` clean.
- CI status: not run this sprint (`.github/workflows/ci.yml` exists; runs on push/PR as usual).

## Failures
None persisted. The test-critic's C-001/C-002 findings identified real coverage gaps (not just
narrow nitpicks) — both closed before this report was written: `merge_backend_opts` extracted as a
shared, directly-testable function (4 new unit tests) and `max_ring`/`stream` config-only
precedence proven at the CLI level (2 new integration tests). See `critique.md` for the full
7-concern record and each response.

## Technical Debt Identified
- **T-3806's locked EARS clause reads "a `Note` **SHALL** be traced" with no `ferric query`-only
  qualifier**, but `ferric mcp`'s `McpServer::launch` only `eprintln!`s `Animus.md`'s presence (no
  trace sink exists at MCP launch time — matches the pre-existing `prompt_composition_error`
  treatment at the same call site). `build-plan.md` is `Finalized — DO NOT EDIT`, so this is left as
  documented drift between the locked plan's literal wording and as-shipped reality (test-critic
  C-004) rather than edited — the authoritative, unlocked artifacts (`integration-tests.md`,
  `decisions.md` ADR-048, `completed-tasks.md`) are all consistent and correct about the deviation.
- **`ferric init-project` (a scaffolding wizard for `.ferric/config.toml`/`Animus.md`)** remains an
  explicit, ADR-048-recorded follow-on — v1 only reads an existing, hand-authored file, same as
  `CLAUDE.md` needs no wizard.
- **`ferric server`/`ferric bench`/`ferric toolbench` are NOT config-defaulted** this sprint —
  scoped to `ferric query`/`ferric mcp` only, a deliberate ADR-048 boundary, not an oversight.

## Coverage Observations
- Every EARS clause in the locked `build-plan.md` now has a corresponding test, including the
  plan-critique's own hardening (the `model_key` derivation fix, the malformed-layer diagnostic
  format, the env-injectable path-resolution branches) and the test-critique's coverage-gap
  additions (`BackendOpts`'s own merge, `max_ring`/`stream` config-only precedence).
- **A genuine masking-hazard bug class was caught TWICE this sprint before shipping**: the plan
  phase's `model_key` finding, and a self-discovered, structurally identical `BackendOpts.backend`
  leftover clap default found while implementing the fix. The test phase's C-001 finding was a
  THIRD instance of the same underlying risk pattern — not a new bug, but a coverage gap that left
  the second fix "correct by inspection" rather than proven. All three are now closed with
  regression tests, and `merge_backend_opts`'s extraction into one shared, tested function
  (replacing two duplicated inline merges) makes a fourth silent recurrence structurally harder,
  not just individually tested away.
- The `--mock` CLI-observability boundary (C-007 from the plan phase, C-003 from the test phase) is
  now precisely and honestly scoped in `integration-tests.md`: `BackendOpts` fields have zero
  CLI-observable effect under `--mock`, so their precedence is proven via dedicated unit tests on
  `merge_backend_opts` instead of CLI subprocess tests — and that boundary is now backed by real
  tests, not assertion by inspection.
- The two "considered deviations" this sprint recorded (mcp's malformed-config diagnostics and
  `Animus.md`'s Note-tracing being `eprintln!`-only, not `Note`-traced, at MCP launch time) were
  independently re-verified by the test-critic against the actual source and confirmed honest,
  well-reasoned scope notes — not hidden gaps.

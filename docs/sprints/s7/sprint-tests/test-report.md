# Sprint 7 Test Report

Sprint goal: realign the code to the harness-owned constrained-decoding thesis
(restore the `Constraint`, enforce it on the HTTP valve, delete the PyO3
backend, make capabilities honest, rebuild the toolbench). Tests were written
TDD-style from each task's EARS clauses and run green in every per-task gate.

## Summary
- **Unit + integration (default graph): 122 passed / 0 failed / 122 total.**
  `cargo test --workspace` does not label suites unit vs integration; the
  s7-new tests within this total are itemized in `unit-tests.md` (21 unit) and
  `integration-tests.md` (~15 integration), all ✅.
- **`backend-openai` feature: 3 passed / 0 failed** — the `build_body`
  `response_format` shape + honest capability flags (`cargo test -p ferric-cli
  --features backend-openai`).
- **E2E: 1 passed (AI-verifiable system test) / 0 failed; real-model acceptance
  N/A (human-heartbeat-gated).** `mock_query_end_to_end` exercises the full
  loop→trace→guard→registry path with zero model. The real-model runs
  (`e2e-tests.md` E2E-1..E2E-3) require a human-launched server / GGUF.
- **CI status: local CI-equivalent GREEN; remote pending push.**
  - `cargo fmt --check` ✅
  - `cargo clippy --all-targets -- -D warnings` (default) ✅
  - `cargo clippy -p ferric-cli --features backend-mistralrs --all-targets -- -D warnings` ✅ (3m44s; compiles clean against the bumped mistralrs **0.8.15**)
  - `cargo clippy -p ferric-cli --features backend-openai --all-targets -- -D warnings` ✅
  - `cargo test --workspace` → 122/0 ✅
  - `aarch64-unknown-linux-gnu` check: not run locally (target not installed) — CI-gated; the sprint adds no new default-graph dependency, so the portability gate is unaffected by construction.
  - GitHub Actions not triggered (branch `sprint-7-realign` is local, not pushed — push/PR is a Loop-phase / user decision).

## Failures
None. The only red encountered mid-sprint was inherited from the failed s6
(corrupted `.gitignore`, trailing-whitespace + ungated imports in the toolbench,
a too-strict `cli.rs` assertion) — all root-caused and fixed, not patched
(see T-001 and T-007 in `completed-tasks.md`).

## Technical Debt Identified
- **mistral.rs in-process *constrained* path stays blocked** on the upstream
  llguidance/mistral.rs hang (ADR-020; tokenizer.json fix DISPROVEN). It is
  honestly routed to `TextXml` and the constrained thesis lives on the HTTP
  valve. Re-enabling it is a tracked backlog item (minimal upstream repro / dep
  bump watch — note the dep already moved 0.8.1→0.8.15, which is a re-test cue).
- **`Constraint::Regex` / `Constraint::Lark`** are carried by the type but only
  `JsonSchema` is wired through the loop (and `Lark`→llama.cpp `grammar`,
  `Regex`→unhandled in the HTTP backend). The loop only ever emits `JsonSchema`
  today; the others are reserved surface, not dead — flagged, not faked.
- **mistral.rs doc comments still cite the 0.8.1 API**; the dep now resolves
  0.8.15 and compiles clean, so the comments are stale but not wrong-in-effect.

## Coverage Observations
- Every T-001..T-004 + T-007 build-plan EARS clause maps to a value-asserting
  test; negative paths are covered (validate constraint+tools, parse
  non-object/missing-tool/missing-args, no-action miss, truncation-vs-parse).
- The honesty invariants are tested directly: `ConstrainedJson` emits
  `constraint_applied` AND carries a real `Constraint` on the request;
  `TextXml` emits neither — the false-`ConstraintApplied` regression cannot recur.
- **Coverage boundary (the heartbeat):** model-free tests prove the loop
  *handles* constrained output; they cannot prove a real server *enforces* it.
  That is E2E-1's job and the ADR-009 merge gate — see `e2e-tests.md` and
  `critique.md` C-002. This is the sprint's stop checkpoint.

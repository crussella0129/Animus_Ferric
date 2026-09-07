# Sprint 125 — End-to-End Test Results

**Status: possible, and exercised.** These drive the full human session
(`session_with` → `Preparation::begin` → `choose_model` → prepared session)
through the scripted-IO fixture, with a real in-process fixture engine, so the
0/1/>1 rule and the decoupled discovery are proven end to end, not only at the
unit/integration layers. Runner: `cargo test -p ferric-cli --locked --lib`.

## The 0/1/>1 rule through a real session

| Test | EARS clause | Assertion | Result |
|------|-------------|-----------|--------|
| `human::enabled::tests::human_journey_e2e_matrix` (absent-dir case) | WHEN 0 GGUFs THEN stop, no engine | A run folder with no models directory → `session_with(...).is_err()` and the fixture port stays `0` (no process started). | ok |
| `human::enabled::tests::human_stale_single_model_still_auto_picks` | WHEN 1 GGUF THEN auto-pick, no picker, no nag | 1 model, saved preference deliberately made stale (the GGUF's bytes are mutated after the first run) → the session runs, **no** "Which model" prompt and **no** "saved model choice changed" message. | ok |
| `human::enabled::tests::human_repeat_with_multiple_models_always_reasks` | WHEN >1 THEN the picker is shown even with a saved preference | 2 models, a preference saved on the first run → the second run still shows a "Which model" prompt (a saved choice is at most a highlight, never a silent skip). | ok |

## Discovery decoupled from the working folder (the core fix)

| Test | EARS clause | Assertion | Result |
|------|-------------|-----------|--------|
| `human::enabled::tests::human_external_models_dir_end_to_end` | WHEN `--models-dir` points outside the run folder THEN the session discovers models there | The run/workspace folder holds **no** `models/`; `--models-dir` points at a separate external temp dir with one GGUF. The full session succeeds (auto-picks the external model), the run folder still has no `models/` of its own, and no picker prompt appears. This is the end-to-end proof that model discovery no longer requires running inside the repo — the exact failure from the second human test. | ok |

This E2E uses the `--models-dir` **flag** rather than mutating the process-global
`FERRIC_MODELS_DIR`, so it composes with the parallel suite without shared-env
flake. The env-var layer itself is proven deterministically by the unit test
`resolve_models_dir_precedence` (which injects an env accessor), and the external
directory's cap-std confinement by the integration tests — so the flag-driven
E2E plus those two together cover the whole `--models-dir` > `FERRIC_MODELS_DIR`
> config > default chain end to end.

## Documented entry point (T-12503)

| Test | EARS clause | Assertion | Result |
|------|-------------|-----------|--------|
| `getting_started_describes_installed_flow` (`tests/getting_started_doc.rs`) | WHEN getting-started is read THEN it describes install / project-folder / `FERRIC_MODELS_DIR` (no "run cargo r in the repo") | `docs/getting-started.md` contains `cargo install --path crates/ferric-cli`, `FERRIC_MODELS_DIR`, and "just run `ferric`", and does **not** contain "Run cargo r in a terminal". | ok |

`welcome()`'s runtime strings (the other half of the T-12503 EARS clause) are
covered by the CLI's own tests; this doc-content check is the ratchet that keeps
the prose guide from drifting back to the superseded in-repo `cargo run` flow.

## Behavior preservation and platform matrix

Full `cargo test --workspace --locked` is green at the tested head (see
`test-report.md`). The heavier feature/target shapes — `lifecycle-fixture`,
`--no-default-features`, and the `aarch64-unknown-linux-gnu` `cargo check` —
are the CI matrix's authoritative jobs and are recorded in `test-report.md`
against the sprint PR's CI conclusion.

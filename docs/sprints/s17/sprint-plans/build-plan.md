Finalized - DO NOT EDIT

# Sprint 17 Build Plan — Durable promotion: read the model profile back

Close the persistence loop: `model_profiles.json` is written by `ferric bench` but
never read. Persist the calibrated ring + read the profile back at `query` time so
a proven model auto-runs at its earned tier + ring. Safe no-op without the file;
`--max-ring` still overrides. Rationale: `sprints/s17/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** the model profile becomes a real input to `query`.
  - **A. Read-back primitive** — T-1701
  - **B. Persist + apply** — T-1702

## Execution Sequence

### T-1701: Profile read-back primitive (ferric-bench)
- **Touches:** `crates/ferric-bench/src/calibrate.rs`, `crates/ferric-bench/src/lib.rs`
- **Success (EARS):**
  - `ModelProfileRecord.calibrated_ring: Option<u8>` — additive `#[serde(default)]`; `calibrate()` sets `None`.
  - `read_profile(dir, model, protocol) -> Option<ModelProfileRecord>` (exact (model,protocol); missing→None).
  - `write_calibrated_ring(dir, model, protocol, params_b, ring)` — load-or-create, set only the ring, preserve `measured_level`.
- **Reuse:** `write_profile` read-merge-write (`calibrate.rs:50`). Tests: round-trip; missing→None; ring-merge keeps measured_level; old JSON → ring None.

### T-1702: Persist + apply the profile (CLI)
- **Touches:** `crates/ferric-cli/src/{toolbench_cmd.rs,query.rs}`, `crates/ferric-cli/tests/cli.rs`, `decisions.md`, `README.md`, `docs/testbench.md`
- **Success (EARS):**
  - `toolbench --calibrate-rings` writes each model's recommended ring via `write_calibrated_ring` (`--profile-dir`, default `benchmarks`).
  - `query --profile-dir` (default `benchmarks`): a record for (model name, protocol label) seeds `measured_level` + defaults `policy.max_ring` to `calibrated_ring`; `--max-ring` overrides; `--mock`/no-file ⇒ unchanged.
  - ADR-029.
- **Notes:** model name from `backend_opts` (skip `--mock`); protocol label matches the writer's `{protocol:?}`.

## Post-build (test)
- ferric-bench unit + the `--mock` read-back CLI test (with-file changes / no-file unchanged) + E2E `--calibrate-rings` writes `calibrated_ring: 1`.

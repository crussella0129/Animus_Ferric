# Sprint 123 Build Plan

## Intents
- [INT-0009](../../../intents/INT-0009-lean-decomposed-architecture.md) — state: planned; acceptance criteria covered: AC-1 (library extraction + thin binaries + duplicate-source warning gone + fixture identity preserved).
- [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) — state: active; T-12028 (duplicate-source build warning) closed as a consequence of INT-0009 AC-1; no INT-0008 text change.

## Schema Tree
- Sprint Goal: ferric-cli is a library with thin binaries
  - Library boundary + shims + identity seam + warning fix
    - T-12301: extract the library (atomic)

## Execution Sequence

### T-12301: Extract `ferric-cli` into a library with thin binary shims
- **Intent:** [INT-0009](../../../intents/INT-0009-lean-decomposed-architecture.md) (AC-1); closes [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) T-12028
- **Touches:** `crates/ferric-cli/src/lib.rs` (new), `crates/ferric-cli/src/main.rs`, `crates/ferric-cli/src/bin/ferric_lifecycle_test.rs` (new), `crates/ferric-cli/src/bin_identity.rs` (new), `crates/ferric-cli/src/tailscale_localapi.rs`, `crates/ferric-cli/Cargo.toml`
- **Depends on:** (none)
- **Acceptance criterion:** INT-0009 AC-1 — a library owns the command surface; binaries are thin shims; no source file is claimed by two build targets (warning gone); the lifecycle fixture's binary-identity behavior is preserved.
- **Success criterion (EARS):**
  - **WHEN** `cargo build -p ferric-cli` runs, **THEN** no "found to be present in multiple build targets" warning **SHALL** be emitted.
  - **WHEN** `name_is_lifecycle_fixture("ferric-lifecycle-test")` is called, **THEN** it **SHALL** return `true`; for any other name it **SHALL** return `false`.
  - **WHEN** the lifecycle fixture spawns the `ferric-lifecycle-test` binary, **THEN** its fixture LocalAPI transport **SHALL** activate exactly as before.
  - **WHEN** the full workspace suite runs after the extraction, **THEN** it **SHALL** pass unchanged.
- **Notes:** atomic — the crate must compile as a unit, so this is one task. `src/lib.rs` holds the ~28 `mod`s + `Cli`/`Command` + `dispatch`/`resolve_cli` + `pub fn run(bin_name: &str)`; `main.rs` and `bin/ferric_lifecycle_test.rs` are shims calling `ferric_cli::run(env!("CARGO_BIN_NAME"))`. `bin_identity.rs` replaces the library-invalid `env!("CARGO_BIN_NAME")` at `tailscale_localapi.rs:219` with a process-set-once identity (`run` calls `set_binary_name` first). Behavior-preserving: the existing `tests/server_lifecycle_fixture.rs` is the load-bearing regression proof.

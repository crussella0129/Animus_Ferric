# Sprint 125 Build Plan

## Intents
- [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) — state: active; advances the human entry point: run-from-anywhere + configured model discovery + honest 0/1/>1 selection (the acquisition clause of AC-13 stays deferred by owner decision).

## Schema Tree
- Sprint Goal: deployable, run-from-anywhere front door
  - Configured model discovery
    - T-12501: models_dir resolution + decoupled discovery
  - Honest selection
    - T-12502: 0/1/>1 picker rule + fit
  - Deploy
    - T-12503: install-once/run-anywhere docs

## Execution Sequence

### T-12501: Configured models directory, decoupled from the workspace
- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/src/config.rs`, `crates/ferric-cli/src/startup/models.rs`, `crates/ferric-cli/src/startup.rs`, `crates/ferric-cli/src/human.rs`
- **Depends on:** (none)
- **Acceptance criterion:** the human entry point works run-from-anywhere — model discovery resolves from a configured location, not the current folder, with the security confinement preserved.
- **Success criterion (EARS):**
  - **WHEN** `resolve_models_dir` is called with no flag, no `FERRIC_MODELS_DIR`, and no config `models_dir`, **THEN** it **SHALL** return `<workspace>/models`.
  - **WHEN** any of flag / `FERRIC_MODELS_DIR` / config `models_dir` is set, **THEN** it **SHALL** win in that precedence order.
  - **WHEN** discovery runs against a resolved directory outside the workspace containing GGUFs, **THEN** it **SHALL** enumerate those GGUFs confined to that directory and **SHALL** reject a symlink or non-file entry.
  - **WHEN** discovery runs with the default `<workspace>/models`, **THEN** it **SHALL** behave identically to the pre-sprint code (existing `startup/models.rs` tests pass unchanged).
- **Notes:** `resolve_models_dir` is a pure function over `(flag, env, config, workspace)` for unit testing; `DiscoveredDirectory::open` opens the resolved dir as its own cap-std root (canonicalize + ambient open), reusing the existing symlink/non-file/entry-cap checks. Read-only discovery.

### T-12502: Owner's picker rule and honest selection
- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/src/human.rs`
- **Depends on:** T-12501
- **Acceptance criterion:** model selection is honest and never silently reuses a stale/oversized model; the human is asked whenever there is a real choice.
- **Success criterion (EARS):**
  - **WHEN** the resolved models dir has 0 GGUFs, **THEN** the session **SHALL** stop with an actionable message that names the resolved directory (not a generic "this folder").
  - **WHEN** it has exactly 1 GGUF, **THEN** that model **SHALL** be auto-selected with its fit surfaced (a won't-fit still requires the confirmation).
  - **WHEN** it has more than 1 GGUF, **THEN** the picker **SHALL** list every model with its fit annotation and require an explicit choice, even when a saved preference exists (no silent skip).
- **Notes:** reuse `fit_annotation` / `wontfit_confirm_prompt` (sprint 122); the `preferred_index` silent-return is removed in favor of the count-based rule; a saved preference may pre-highlight but never bypass the list.

### T-12503: Install-once, run-anywhere deploy story
- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/src/human.rs` (`welcome()`), `docs/getting-started.md`
- **Depends on:** T-12501, T-12502
- **Acceptance criterion:** the documented entry point is the installed binary run from the user's project folder with a configured models dir — not `cargo r` in the repo.
- **Success criterion (EARS):**
  - **WHEN** `welcome()` and `docs/getting-started.md` are read, **THEN** they **SHALL** describe `cargo install --path`, running `ferric` in the user's project folder, and pointing `FERRIC_MODELS_DIR` at a GGUF folder — with no instruction to run `cargo r` inside the repo.
- **Notes:** the ferric-source-tree Work guard stays as the backstop for a stray in-repo `cargo run`. Template hygiene: docs use `FERRIC_MODELS_DIR=<your gguf folder>`, no concrete machine path.

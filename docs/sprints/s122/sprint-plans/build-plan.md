Finalized - DO NOT EDIT

# Sprint 122 Build Plan

## Intents
- [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) — state: active; acceptance criteria covered: AC-13 (hardware-informed recommendation clause — the RAM-fit increment; acquisition and GPU/VRAM clauses remain active follow-on work).

## Schema Tree
- Sprint Goal: hardware-informed model fit in the front door
  - Platform probe
    - T-12201: memory probe seam
  - Fit model (pure)
    - T-12202: estimate + classify
  - Front-door surfaces
    - T-12203: picker annotation + won't-fit confirmation

## Execution Sequence

### T-12201: Best-effort system-memory probe behind an injectable seam
- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/src/startup/memory.rs` (new), `crates/ferric-cli/src/startup.rs` (module decl), `crates/ferric-cli/Cargo.toml` (windows-sys dev-dep → `cfg(windows)` dep + `Win32_System_SystemInformation`)
- **Depends on:** (none)
- **Acceptance criterion:** AC-13 — "hardware-informed recommendations"; a recommendation needs a measured, non-fabricated memory reading, with "unknown" a valid outcome.
- **Success criterion (EARS):**
  - **WHEN** `parse_meminfo` is given standard `/proc/meminfo` text, **THEN** it **SHALL** return `SystemMemory` with `MemTotal` and `MemAvailable` converted from kB to bytes.
  - **WHEN** the text lacks a `MemAvailable` line, **THEN** `parse_meminfo` **SHALL** return `None` rather than a fabricated or zero value.
  - **WHEN** `NativeMemoryProbe::probe` runs on this host, **THEN** it **SHALL** return `Some(SystemMemory)` with `total_bytes > 0`.
- **Notes:** `MemoryProbe` trait mirrors the existing `HumanIo`/`Preparation` injection seams so the surfaces are testable without real hardware. Windows `GlobalMemoryStatusEx`, macOS `sysctl HW_MEMSIZE` via the already-present `libc`. No new crate (ADR-004).

### T-12202: Pure model-memory estimate and fit classification
- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-core/src/fit.rs` (new), `crates/ferric-core/src/lib.rs` (re-export)
- **Depends on:** (none)
- **Acceptance criterion:** AC-13 — the recommendation must be honest about whether a model can run, and unmeasured remains explicitly unmeasured.
- **Success criterion (EARS):**
  - **WHEN** `estimate_model_memory(file_bytes, context)` is called, **THEN** it **SHALL** return a value ≥ `file_bytes` plus a positive context allowance.
  - **WHEN** `classify_fit(estimate, Some(available))` has `estimate` within the safe margin of `available`, **THEN** it **SHALL** return `Fits`; within the tight band `Tight`; when `estimate > available`, `WontFit`.
  - **WHEN** `classify_fit(estimate, None)` is called, **THEN** it **SHALL** return `Unknown`.
  - **WHEN** a 15.3 GiB model is classified against a small available RAM (the human-test case), **THEN** `classify_fit` **SHALL** return `WontFit`.
- **Notes:** headroom is a named constant with a doc-comment stating it models weights + a context KV allowance, not a byte-exact predictor. Pure functions in `ferric-core` (no platform, no deps) so the aarch64 gate is unaffected.

### T-12203: Honest picker annotation and won't-fit confirmation
- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/src/human.rs` (choose_model, the engine-start gate at the `will_start_engine` block, and threading `Option<SystemMemory>` in)
- **Depends on:** T-12201, T-12202
- **Acceptance criterion:** AC-13 — hardware-informed recommendation surfaced to the human without a settings questionnaire; the 27B-on-CPU trap cannot happen silently.
- **Success criterion (EARS):**
  - **WHEN** the picker lists models and memory is known, **THEN** each entry **SHALL** be annotated with its `Fit` classification.
  - **WHEN** the selected model classifies as `WontFit` in an interactive session, **THEN** the engine-start step **SHALL** require an explicit additional confirmation that names the estimate and available memory before starting.
  - **WHEN** memory is `Unknown`, **THEN** the surface **SHALL** preserve the current caution line and add no fabricated numbers.
- **Notes:** reuse the existing `ScriptedIo` seam and add an injected memory value so the picker/gate are deterministically testable; the `WontFit` confirmation reuses the `io.read` y/N pattern already used for the source-tree guard and engine start.

# Sprint 38 Meta

- **Sprint number:** 38
- **Start timestamp:** 2026-07-04T16:04:18Z
- **End timestamp:** 2026-07-04T21:15:00Z
- **Model:** claude-sonnet-5
- **Exit status:** success
- **Token count:** (not observable in this harness)
- **Summary:** Persistent layered configuration (`.ferric/config.toml` project + cross-platform
  user config, CLI flag > project > user > hardcoded default) for `ferric query`/`ferric mcp`,
  plus `Animus.md` (a freeform project-instructions file folded into the system prompt). Caught
  and fixed a masking-hazard bug class (config-resolved values invisible to the code deciding
  behavior) three times across the plan/build/test phases — `model_key`, `BackendOpts.backend`,
  and the `merge_backend_opts` coverage gap. ADR-048. All 7 build tasks + the test-phase coverage
  fixes shipped; `cargo test --workspace` green throughout.

Finalized - DO NOT EDIT

# Sprint 38 Test Plan

## Unit Tests

### T-3801 unit tests
- `load_layered_from_project_only`: only a project config → its fields populate `LoadedConfig.config`,
  `diagnostics` empty.
- `load_layered_from_user_only`: only a user config → its fields populate `LoadedConfig.config`,
  `diagnostics` empty.
- `load_layered_project_wins_on_overlap`: both set the same field → project's value wins.
- `load_layered_neither_present_is_all_none`: neither file exists → all-`None` `Config`, empty
  `diagnostics`, no error.
- `load_layered_malformed_toml_pushes_diagnostic`: **(C-004)** a malformed project config file →
  that layer treated as absent, and `LoadedConfig.diagnostics` contains exactly one string naming
  the offending path (asserted directly on the returned data, not by capturing stderr); a valid
  user layer beneath it still applies.
- `project_config_path_is_workspace_relative`: `<workspace>/.ferric/config.toml`.
- **(C-003)** `user_config_path_from_appdata_only`: an injected env closure resolving only
  `APPDATA` → the Windows-shaped path (`<APPDATA>/ferric/config.toml`).
- `user_config_path_from_xdg_only`: only `XDG_CONFIG_HOME` resolves → the XDG-shaped path.
- `user_config_path_from_home_fallback_only`: only `HOME` resolves (neither `APPDATA` nor
  `XDG_CONFIG_HOME`) → the `.config`-fallback path.
- `user_config_path_from_nothing_resolves_to_none`: the closure returns `None` for every key →
  `None`.
- `user_config_path_wrapper_uses_real_env`: a light smoke test that the real `user_config_path()`
  wrapper delegates to `user_config_path_from` (shape check only — not asserting the exact
  real-machine value, which varies by CI runner).
- Stubs: `tempfile::tempdir()` for project/user config file locations; no real `~/.config`/
  `%APPDATA%` touched by any test (the env-injectable `_from` variant means NO test needs to mutate
  real process env vars).

## Integration Tests
### T-3802/T-3804 — mechanical clap-default removal (behavior-preserving)
- `cli::query_defaults_unchanged_after_clap_type_change`: no CLI flags for any of the six fields,
  no config file present → `RunConfigArgs`' resolved values match today's hardcoded constants
  exactly (isolates the refactor from config logic; if this fails, the regression is in T-3802, not
  T-3803).
- `mcp::launch_defaults_unchanged_after_clap_type_change`: same, scoped to `ferric mcp` (T-3804).

### T-3803/T-3805 — config loading + precedence resolution
- `cli::config_file_sets_default_without_flag`: a `.ferric/config.toml` sets e.g. `params_b`; run
  `ferric query --mock` with NO `--params-b` flag; confirm the config value took effect (via the
  trace's recorded tier/offered-tools, mirroring `max_ring_caps_the_offered_tools`'s existing
  trace-inspection pattern).
- `cli::cli_flag_overrides_config_file`: the same config file set, but `--params-b` ALSO passed
  with a different value; confirm the CLI flag's value wins.
- **(C-001)** `cli::config_only_model_still_resolves_profile`: a `.ferric/config.toml` sets `model`
  (or `model_file`) to a value with an existing calibration profile record, NO matching CLI flag;
  assert the trace shows the profile was found and `measured_level`/`calibrated_ring` applied —
  proving `model_key` was derived from the config-resolved value, not skipped. This is the direct
  regression test for the concern the plan-critic raised; without the T-3803 fix, this test fails
  (profile lookup silently skipped).
- **(C-004)** `cli::malformed_config_traced_as_note`: a malformed `.ferric/config.toml`; run
  `ferric query --mock`; assert a `Note` event appears in the trace carrying the diagnostic (not
  just checked via captured stderr).
- `mcp::config_file_sets_default_without_flag` / `mcp::cli_flag_overrides_config_file` /
  `mcp::config_only_model_still_resolves_profile` / `mcp::malformed_config_traced_as_note`: the same
  four cases as above, via an in-process `McpServer::launch` call (since `ferric mcp` isn't
  naturally probed via a single subprocess call+exit).
- **(C-007) Scope note:** `--mock`'s `build_run_config` branch never reads `BackendOpts` fields
  (`backend`/`model`/`model_dir`/`model_file`/`api_base`/`api_key`) and never calls
  `create_provider` — so config-set `BackendOpts` fields have NO CLI-observable effect under
  `--mock`. The tests above deliberately probe `params_b` (a `ModelProfile`/tier-affecting field,
  observable via the trace) for the CLI-level precedence proof. `BackendOpts`' own
  `.or(config.field)` merge logic is instead covered by direct unit tests on the resolution
  function in isolation (no CLI subprocess, no `--mock` involved) — this is the ONLY way those
  fields' precedence gets tested, and is called out explicitly so the gap isn't mistaken for
  untested code.
- **Regression:** every existing `ferric query`/`ferric mcp` test (run with no `.ferric/config.toml`
  present in their tempdir workspaces) continues to pass unchanged — proves the "no config, no
  flags → byte-identical" clause for free.

## End-to-End Tests
- **Status:** possible (via `--mock`, no real GGUF model required).
- `cli::animus_md_folds_into_prompt`: an `Animus.md` file at the workspace root; run `ferric query
  --mock`; assert its content reached the assembled prompt (via the trace's `prompt_assembled` char
  count, mirroring the existing `query_file_text_folds_into_prompt` test's technique).
- **(C-005)** `cli::animus_md_present_traces_note`: the same `Animus.md`-present setup; assert a
  `Note` event appears in the trace recording that it was applied (the EARS clause is scoped to
  presence only — absence is deliberately not traced, matching the untraced default "no
  `prompts_dir` configured" path).
- `cli::animus_md_absent_is_unchanged`: covered by every existing CLI test already NOT creating an
  `Animus.md` file in their tempdir workspaces — no new test needed, the regression is already
  proven; explicitly, none of those runs should show an Animus-related `Note`.
- A real-backend config/Animus.md smoke (a live llama-server, confirming the resolved model/backend
  actually loads) is a **manual verification step**, not automated — matches the project's
  established no-live-backend-CI position (ADR-045).

## Build / Lint (all tasks)
- `cargo test --workspace` green.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `cargo fmt --all --check` clean.

# Sprint 38 Unit Tests

All derived from the locked `build-plan.md` EARS clauses (one test per WHEN/THEN/SHALL triple),
incl. the plan-critique's C-001/C-003/C-004/C-005 fixes folded in during the build. All green.

## T-3801 — `Config` struct + layered load
- `config::tests::load_layered_from_project_only` — a project-only file populates
  `LoadedConfig.config` with exactly its set fields, empty `diagnostics`.
- `config::tests::load_layered_from_user_only` — a user-only file (absent project path)
  populates the same way.
- `config::tests::load_layered_project_wins_on_overlap` — both set the SAME field → project wins.
- `config::tests::load_layered_neither_present_is_all_none` — neither file exists → all-`None`
  `Config`, empty `diagnostics`, no error.
- `config::tests::load_layered_malformed_toml_pushes_diagnostic` (C-004) — a malformed project
  layer degrades to `None` for that layer AND pushes exactly one diagnostic string naming the
  path — asserted directly on `LoadedConfig.diagnostics`, not captured stderr; a valid user layer
  beneath it still applies.
- `config::tests::project_config_path_is_workspace_relative` — `<workspace>/.ferric/config.toml`.
- `config::tests::user_config_path_from_appdata_only` (C-003) — an injected env closure resolving
  only `APPDATA` → the Windows-shaped path.
- `config::tests::user_config_path_from_xdg_only` — only `XDG_CONFIG_HOME` resolves → the
  XDG-shaped path.
- `config::tests::user_config_path_from_home_fallback_only` — only `HOME` resolves → the
  `.config`-fallback path.
- `config::tests::user_config_path_from_nothing_resolves_to_none` — nothing resolves → `None`.
- `config::tests::user_config_path_wrapper_uses_real_env` — the real `user_config_path()` wrapper
  delegates sensibly (shape check only; no real-machine value asserted).
- `config::tests::backend_arg_config_field_uses_kebab_case_spelling` — `backend = "openai"` in TOML
  deserializes to `BackendArg::Openai`, matching clap's own lowercase `ValueEnum` spelling.

## T-3803/T-3805 — `merge_backend_opts` (test-critic C-001/C-003 fix)
Extracted from what was previously two duplicated inline merges (`query.rs`/`mcp.rs`) into one
shared, directly-testable function — closing the test-critic's finding that the `BackendOpts`
merge (the same masking-hazard class as the `model_key` fix) shipped with no dedicated test, only
"correct by inspection."
- `config::tests::merge_backend_opts_config_only_backend_is_applied` — `backend` set ONLY via
  `Config`, no CLI value → applied.
- `config::tests::merge_backend_opts_cli_flag_wins_over_config` — both set → the CLI value wins.
- `config::tests::merge_backend_opts_config_only_remaining_fields_are_applied` — the same
  config-only precedence for `model_dir`/`model_file`/`model`/`api_base`/`api_key`, in one pass.
- `config::tests::merge_backend_opts_cli_model_wins_over_config_model` — `model` specifically,
  CLI-wins case (the field the ADR-029 `model_key` fix depends on).

## T-3803/T-3806 — `Animus.md` fold (pure helper)
- `query::tests::fold_animus_md_appends_a_distinct_block` — `Animus.md` content is appended after
  an existing base prompt, in a distinct, clearly-delimited block.
- `query::tests::fold_animus_md_falls_back_to_default_prompt` — absent `existing` falls back to
  `DEFAULT_SYSTEM_PROMPT` as the base, matching what the loop itself does when `system_prompt` is
  `None`.

## T-3805 — `ferric mcp` config precedence (in-process `McpServer::launch`)
- `mcp::tests::launch_defaults_unchanged_after_clap_type_change` — all-`None` `McpArgs`, no config
  file → `Tier::Nano` / 512 max output tokens (mirrors `cli::query_defaults_unchanged_...`).
- `mcp::tests::launch_config_file_sets_default_without_flag` — `params_b = 8.0` via config, no
  flag → `Tier::Small`.
- `mcp::tests::launch_cli_flag_overrides_config_file` — the same config set, but `args.params_b`
  ALSO set → the CLI-equivalent field wins.
- `mcp::tests::launch_config_only_model_still_resolves_profile` (C-001) — `model` set ONLY via
  config + a persisted `calibrated_ring: 0` record → `server.config.policy.max_ring == Some(0)`,
  proving `model_key` was derived from the config-resolved value, not skipped.

## Result
`cargo test -p ferric-cli` (default): 64 unit tests passed (up from 41 pre-sprint), incl. all of
the above. `--features backend-openai` / `--features backend-mistralrs`: unaffected, both clean.

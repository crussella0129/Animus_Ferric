# Sprint 38 Integration Tests

Black-box subprocess tests against the real `ferric` binary (`crates/ferric-cli/tests/cli.rs`).

## T-3802/T-3804 — mechanical clap-default removal (behavior-preserving)
- `cli::query_defaults_unchanged_after_clap_type_change` — a `--mock` run with NO flags and NO
  config file → the `policy_selected` trace event shows `tier: nano` / `max_output_tokens: 512`
  (the default `--params-b` 1.2's tier) — byte-identical to before the clap-type refactor, isolated
  from any config-precedence logic (T-3804's mcp-side equivalent is a unit test — see
  `unit-tests.md` — since `ferric mcp` isn't naturally probed via a single subprocess call+exit).

## T-3803/T-3805 — config loading + precedence resolution
- `cli::config_file_sets_default_without_flag` — `.ferric/config.toml` sets `params_b = 8.0`, no
  matching CLI flag → `Tier::Small` (the config value took effect).
- `cli::cli_flag_overrides_config_file` — the same config file, but `--params-b 1.2` ALSO passed →
  the CLI flag's value wins (`Tier::Nano`).
- `cli::config_only_model_still_resolves_profile` (C-001, the plan-critic's most significant
  finding) — `.ferric/config.toml` sets `model` (no `--model` flag) alongside a persisted
  `calibrated_ring: 0` profile record for that model; the offered-tools trace shows Ring 1 dropped
  — proving `model_key` was derived from the config-resolved value, not silently skipped. This is
  the direct regression test for the bug the critic caught before it shipped.
- `cli::malformed_config_traced_as_note` (C-004) — a malformed `.ferric/config.toml` → a `Note`
  event in the trace carries the diagnostic text (not just an unasserted stderr print).
- `cli::config_only_max_ring_caps_the_offered_tools` (test-critic C-002) — `max_ring = 0` set ONLY
  via config, at Small tier (which otherwise offers Ring 1 too) → the cap still applies.
- `cli::config_only_stream_enables_live_output` (test-critic C-002) — `stream = true` set ONLY via
  config, `--protocol grammar --mock` → the raw streamed JSON appears on stdout in place of the
  clean final-echo line, proving the config-only value actually reached `resolved_stream`.
- **Regression:** every pre-existing `ferric query`/`ferric mcp` test (none create a
  `.ferric/config.toml` in their tempdir workspaces) continues to pass unchanged — proves the
  "no config, no flags → byte-identical" clause for free.
- **Scope note (C-007), tightened per test-critic C-003:** `--mock`'s `build_run_config` branch
  never reads `BackendOpts` fields or calls `create_provider`, so those fields have no
  CLI-observable effect under `--mock` — the tests above deliberately probe `params_b`/`max_ring`/
  `stream` (all observable via the trace or stdout) for the CLI-level precedence proof.
  `BackendOpts`' own merge is now covered directly, not "by inspection": extracted into
  `config::merge_backend_opts` (a single function both `run_query` and `McpServer::launch` call,
  replacing what had been two duplicated inline merges) with 4 dedicated unit tests — see
  `unit-tests.md`'s new T-3803/T-3805 section.
- **mcp-side deviation, noted (not a gap):** the test-plan's literal `mcp::malformed_config_
  traced_as_note` case was NOT built. `ferric mcp`'s `McpServer::launch` has no trace sink at
  launch time (each `tools/call` opens its own) — `eprintln!`-only matches the pre-existing
  treatment of `prompt_composition_error` at the same call site. Writing a `Note` into every
  subsequent `tools/call`'s trace would spam rather than inform. See `decisions.md` ADR-048 and
  `agent-tasks/completed-tasks.md`'s T-3805 entry.

## T-3806 — `Animus.md`
- `cli::animus_md_folds_into_prompt` — an `Animus.md` file at the workspace root; its content
  reaches the assembled prompt (via the trace's `prompt_assembled` char count).
- `cli::animus_md_present_traces_note` (C-005, narrowed to presence-only) — a `Note` event
  confirms `Animus.md` was applied.
- **Regression (absence):** every pre-existing CLI test (none create an `Animus.md`) continues to
  pass unchanged, and none show an Animus-related `Note` — proves the absence clause for free.

## Result
`cargo test -p ferric-cli --test cli`: 21 passed (up from 12 pre-sprint). `cargo test --workspace`:
all green. `cargo clippy -p ferric-cli --all-targets` clean on default, `backend-openai`, and
`backend-mistralrs` feature sets. `cargo fmt --all --check` clean.

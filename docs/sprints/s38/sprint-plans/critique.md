# Sprint 38 Plan Critique — Responses

Reviewed by a foreground plan-critic agent against `research-report.md` and actual source
(`query.rs`, `mcp.rs`, `backend.rs`, `server.rs`). Overall verdict: `proceed-with-caveats`, 8
concerns. All applied to `build-plan.md`/`test-plan.md` except C-006 (light-touch) and C-008
(rejected, no change).

### C-001 — `model_key` derivation bypasses config (significant)
**Failure mode:** the plan described merging `BackendOpts` fields generally but never revisited
`model_key`'s derivation (`args.backend_opts.model.clone().or_else(|| args.backend_opts.model_file
.clone())`, feeding `ferric_bench::read_profile`'s ADR-029 profile lookup). Built as originally
worded, a config-only-set `model` (no CLI flag) would leave `model_key` as `None`, silently
skipping profile lookup and losing the earned `measured_level`/`calibrated_ring` promotion with no
error or trace.
**Response:** fix-in-plan. T-3803/T-3805 now explicitly require `model_key` be re-derived from the
POST-merge, config-resolved `model`/`model_file` values, and a dedicated EARS clause + test
(`cli::config_only_model_still_resolves_profile` / the `mcp::` equivalent) directly proves it.

### C-002 — T-3802/T-3803 bundle three concerns (granularity)
**Failure mode:** the original T-3802 (and T-3803) combined a mechanical clap-type change, config
loading, and ~12-field precedence resolution (now also carrying the C-001 fix) into one task —
a regression in any of the three would be hard to attribute.
**Response:** fix-in-plan. Split into T-3802 (mechanical clap-default removal, behavior-preserving,
`ferric query`) / T-3803 (config load + precedence + `model_key` fix, `ferric query`) and T-3804 /
T-3805 (same split for `ferric mcp`). `Animus.md` and docs renumbered to T-3806/T-3807.

### C-003 — `user_config_path` branch coverage (missing-risk)
**Failure mode:** the three real-OS branches (Windows/XDG/HOME-fallback) had only a "shape" sanity
test, not real per-branch coverage, and testing them for real would require mutating actual process
env vars (flaky, order-dependent).
**Response:** fix-in-plan. Added `user_config_path_from(env: &impl Fn(&str) -> Option<String>)`,
mirroring `load_layered_from`'s test-injection pattern; four dedicated unit tests exercise each
branch (APPDATA-only, XDG-only, HOME-fallback-only, none-resolve) via an injected closure, no real
env touched.

### C-004 — malformed-TOML diagnostic isn't testable data (plan-test-mismatch)
**Failure mode:** the EARS clause promised a stderr diagnostic but no test could assert its
content without capturing stderr, which the project doesn't otherwise rely on for test assertions.
**Response:** fix-in-plan. `load_layered_from` now returns `LoadedConfig { config, diagnostics:
Vec<String> }` (mirrors the existing `prompt_composition_error: Option<String>` pattern already on
`RunConfig`); callers `eprintln!` each diagnostic AND thread it into `RunConfig` for Note-tracing.
Both the unit test (`load_layered_malformed_toml_pushes_diagnostic`) and the CLI-level test
(`cli::malformed_config_traced_as_note`) assert on the data, not stderr capture.

### C-005 — `Animus.md` Note-tracing untested for both branches (plan-test-mismatch)
**Failure mode:** the EARS clause required tracing on BOTH presence and absence, but no test
verified a Note event was ever written — the only related test checked `prompt_assembled`'s char
count.
**Response:** fix-in-plan, narrowing per the critic's own suggested resolution. The EARS clause is
narrowed to trace only on PRESENCE (matching the existing precedent that the ordinary default path
— e.g. "no `prompts_dir` configured" — is untraced); a new test
(`cli::animus_md_present_traces_note`) asserts the Note appears when the file is present, and the
existing absent-case regression tests are called out as implicitly proving no Note fires when
absent.

### C-006 — ADR-010 non-interaction unstated (ignored-ADR)
**Failure mode:** ADR-010/015 (constraint/native-tools mutual exclusion) weren't listed in Decisions
Reviewed; plausibly a non-issue since `validate()`/`select_protocol` operate on final resolved
values regardless of origin, but worth confirming explicitly.
**Response:** defer-with-rationale, per the critic's own suggestion — no plan restructuring needed,
just one confirming sentence added to T-3807's ADR-048 description (now present in build-plan.md).

### C-007 — `--mock` scope limitation unstated (e2e-drift)
**Failure mode:** the test-plan's integration tests rely on `--mock`, but `--mock`'s
`build_run_config` branch never reads `BackendOpts` fields or calls `create_provider` — so
config-set `backend`/`model`/`model_dir`/`model_file`/`api_base`/`api_key` have zero CLI-observable
effect under `--mock`. The test-plan already made the right call (probing `params_b`, an observable
tier-affecting field) but didn't say so, which could read as an unacknowledged gap.
**Response:** fix-in-plan. Added an explicit "Scope note" to test-plan.md's integration section
naming the limitation and clarifying `BackendOpts`' own precedence logic is covered by direct unit
tests instead, not CLI-observable ones.

### C-008 — T-3807 (was T-3805) EARS clause is a doc-existence check (EARS-vague)
**Failure mode:** the clause checks that ADR-048 states certain things, not runtime behavior.
**Response:** reject, per the critic's own assessment — this is the established, acceptable pattern
for doc-only tasks in this project (matches sprint 37's ADR-047/T-3706 precedent). No change.

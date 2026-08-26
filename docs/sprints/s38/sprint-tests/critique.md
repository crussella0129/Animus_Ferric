# Test Critique — Sprint 38

Reviewed by a foreground test-critic agent against `build-plan.md` (locked EARS clauses),
`test-plan.md`, `critique.md` (the plan phase's earlier findings), and the just-written
`unit-tests.md`/`integration-tests.md`/`e2e-tests.md`, cross-checked against the actual source.
Overall verdict: `proceed-with-caveats`, 7 concerns. C-001/C-002/C-003 addressed with code + test
changes; C-004 deferred with rationale (locked-plan constraint); C-005/C-006/C-007 rejected as
verification-only findings that confirmed existing claims were honest.

### C-001 — `BackendOpts.backend`'s masking-hazard fix has no dedicated regression test (negative-path/EARS-coverage)
**Failure mode:** ADR-048 and the T-3803 completed-tasks entry both frame the `BackendOpts.backend`
clap-default fix as "the same bug class" as the headline `model_key` fix, serious enough to record
as a pattern worth watching for — yet unlike `model_key` (which got `cli::config_only_model_
still_resolves_profile`), no test anywhere set `backend` ONLY via config and confirmed it reached
`RunConfigArgs.backend`/`create_provider`. The only test touching the field
(`backend_arg_config_field_uses_kebab_case_spelling`) is a pure TOML-deserialization shape check —
it never exercises the merge line itself.
**Response:** add-test, done as a refactor-while-fixing. Extracted the previously-duplicated inline
merge (6 lines each in `query.rs`/`mcp.rs`) into one shared `config::merge_backend_opts(opts,
&cfg) -> BackendOpts`, now the SINGLE call site both surfaces use. 4 new dedicated unit tests:
`merge_backend_opts_config_only_backend_is_applied`, `merge_backend_opts_cli_flag_wins_over_config`,
`merge_backend_opts_config_only_remaining_fields_are_applied` (the other 5 `BackendOpts` fields in
one pass), `merge_backend_opts_cli_model_wins_over_config_model`.

### C-002 — `max_ring`/`stream` are named in-scope for config precedence but never tested at those fields (EARS-coverage)
**Failure mode:** build-plan.md's T-3803 description explicitly lists `max_ring`/`stream` as
resolved the same way as the six clap-default fields, but every concrete precedence test
(`config_file_sets_default_without_flag`, `cli_flag_overrides_config_file`,
`config_only_model_still_resolves_profile`) only instantiated `params_b` or `model`/`model_file`.
Neither `max_ring` nor `stream` had a CLI-observable proof that `cfg.max_ring`/`cfg.stream` actually
reaches `resolved_max_ring`/`resolved_stream` without a matching flag.
**Response:** add-test. `cli::config_only_max_ring_caps_the_offered_tools` — `max_ring = 0` via
config only, at Small tier (which otherwise offers Ring 1), confirms the cap applies (reusing the
existing offered-tools trace-inspection pattern). `cli::config_only_stream_enables_live_output` —
`stream = true` via config only, `--protocol grammar --mock` (where the mock's completion text IS
the raw JSON, unlike the NativeTools mock which has `text: None`); the default `complete_streaming`
fires one `Text` delta of the raw JSON when streaming is active, which prints instead of the clean
final-echo line — a real, config-driven behavioral difference, not just "didn't crash."

### C-003 — the `--mock` `BackendOpts`-merge scope note overclaims "proven by inspection" (stub-leak / weak-assertion in prose)
**Failure mode:** the original integration-tests.md scope note said the merge was "proven correct
by inspection" — for a sprint whose entire thesis is "config values must reach the code that
decides behavior, and we caught two real bugs where they silently didn't," calling an untested code
path "proven" is a materially overstated claim.
**Response:** tighten-assertion, closed together with C-001. The scope note now says the merge "is
now covered directly, not 'by inspection'" and names the 4 new `merge_backend_opts` unit tests —
the overclaim is gone because the underlying gap is gone.

### C-004 — T-3806's EARS clause doesn't scope Note-tracing to `ferric query` only, but mcp.rs only `eprintln!`s (EARS-coverage, in letter only)
**Failure mode:** build-plan.md's T-3806 EARS clause reads "a `Note` **SHALL** be traced" with no
`ferric query`-only qualifier, yet `McpServer::launch` only `eprintln!`s `Animus.md`'s presence (no
sink exists at MCP launch time). The critic judged this an honest, well-reasoned deviation (matches
the pre-existing `prompt_composition_error` treatment at the same call site) rather than a hidden
gap, but noted the locked EARS clause's literal text doesn't say so.
**Response:** defer-with-rationale. `build-plan.md`/`test-plan.md` are locked ("Finalized - DO NOT
EDIT") — per the sprint-loop protocol, locked plan files are not retroactively edited. The
clarification lives here instead, plus it was already recorded in `decisions.md` ADR-048 and
`agent-tasks/completed-tasks.md`'s T-3806/T-3805 entries at build time. No code change needed; the
shipped behavior is correct, only the plan's literal wording is slightly broader than what was
built — a documentation-precision gap, not a behavior gap.

### C-005 — the mcp-side `malformed_config_traced_as_note` omission, re-verified (verification only)
**Failure mode:** none — the critic independently re-checked `mcp.rs:451-460` against the
integration-tests.md's own disclosure and confirmed it's accurate: `eprintln!`-only, no sink, no
`Note`, matching `prompt_composition_error`'s pre-existing treatment.
**Response:** reject (as a concern) — confirmed honest, no action.

### C-006 — the e2e-tests.md "integration-level e2e already" claim, re-verified (verification only)
**Failure mode:** none — the critic independently confirmed the three named tests really do spawn a
real `ferric` subprocess, write real files to a real tempdir, and read back a real trace file, with
only the model backend itself mocked (the separately-disclosed ADR-045 boundary).
**Response:** reject (as a concern) — confirmed honest, no action.

### C-007 — flake-risk sweep (verification only)
**Failure mode:** none found — no real env mutation (the injected-closure design specifically
avoids this), no shared filesystem state (every test uses its own `tempfile::tempdir()`), no
timing/ordering assumptions introduced this sprint.
**Response:** reject (no concern) — screened, nothing concrete surfaced.

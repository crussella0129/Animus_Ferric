# Sprint 130 — Test report

## How each retirement was verified

Each retirement landed as its own commit. Before each one:

- `cargo fmt --all -- --check` passed;
- clippy passed with `-D warnings` across the workspace, the lifecycle
  fixture, the Windows target and, until R-5, the no-default-features
  build;
- `cargo test --workspace --no-fail-fast` passed, apart from the same 19
  ferric-cli lib lifecycle tests that fail in this sandbox but pass on
  GitHub;
- a before/after test-inventory diff was taken.

CI was watched after **every** push; every job was green on each commit.

## Size

| Crate | After sprint 129 | After sprint 130 |
|---|---:|---:|
| ferric-cli | 69,245 | 52,092 |
| ferric-loop | 19,085 | 18,825 |
| ferric-bench | 8,683 | 7,538 |
| ferric-research | 2,316 | 1,978 |
| ferric-trace | 2,014 | 1,948 |
| all others | 30,078 | 30,076 |
| **Total Rust** | **131,421** | **112,457** |

- **Sprint 130:** −18,964 lines.
- **Since the start of sprint 129:** 140,082 → 112,457 (−27,625, −19.7%),
  and 20 → 14 crates.
- **Test names:** 1,414 → 1,309.

## Per retirement

| Step | Commit | Net lines | Tests that guard what remains |
|---|---|---|---|
| R-8 tailnet retriever, R-9 tools installers | `b19a725` | −280 | the remaining research tests; `getting_started_doc` |
| R-1 Tailscale Serve, R-2 schema-1 and `server adopt` | `e42778f` | −14,700 | See note 1. |
| R-3 paired autonomy A/B | `810d0b5` | −3,560 | See note 2. |
| R-6 EvidencePlanner, R-7 pre-sprint-113 replay | `598c2c7` | −400 | See note 3. |
| R-5 backend-free build | `1e7140f` | −280 | `source_execution` (no feature can drop startup; no backend-free lane); a normal build is byte-identical |

Notes on the guarding tests:

1. **R-1/R-2:** retired records are refused at capture and never signalled
   or deleted. Guards:
   - `capture_reports_retired_records_for_manual_cleanup`;
   - `retired_registrations_block_status_down_and_doctor_without_effects`;
   - `live_retired_registrations_are_never_signalled_or_deleted`;
   - the end-to-end `retired_registrations_block_cli_without_signal_or_delete`.
2. **R-3:** the single-binary matrix is unchanged. Guards:
   - `single_binary_parser_keeps_only_the_legacy_child_policy`;
   - `retained_trace_name_keeps_the_single_legacy_layout`;
   - `pre_retirement_paired_and_single_rows_still_parse`.
3. **R-6/R-7:**
   - `evidence_runs_and_planner_fails_before_trace_or_workspace_mutation`:
     the planner value is now rejected by clap.
   - `pre_sprint_113_trace_is_refused`: old traces fail closed instead of
     replaying silently.
   - Trace-equivalence capture: across the loop and CLI suites, the only
     differences are the deliberately rewritten fixtures.

Every removed test went with the feature it tested. The commit messages list
them by suite.

## Not done: R-4 (the Legacy `Tool::run` path)

Before removing anything, the agent ran every builtin tool both ways on
identical workspaces: Legacy `Registry::execute`, and prepare/commit. Only
9 of 39 scenarios matched.

| Area | Legacy | Prepare/commit |
|---|---|---|
| Missing parent directories | Created | Refused |
| Identical content | Written | Refused as a no-op |
| Syntax-breaking writes | Written with a warning | Refused |
| Moving onto an existing file | Overwritten | Refused |
| Reads | Plain text | Observation headers added |
| Error texts | | Differ |
| Opaque tools (`git_read`, `git_write`, `fetch_reference`, `shell_exec`, `manage_task`) | Run | Cannot be prepared |

So retiring `Tool::run` would change what the default policy does to files.
That is a decision about the Legacy policy itself (T-13006), not removal of
a duplicate.

## Remaining sandbox-only failures

The 19 ferric-cli lib lifecycle tests, and `model_free_server_lifecycle_fixture_e2e`,
fail here because PID 1 cannot be inspected in this sandbox. They pass on
GitHub (in a private PID namespace). T-12907 records the wider Linux
non-root issue.

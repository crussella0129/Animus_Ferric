# Sprint 129 — Test report

## How each step was verified

Every area landed as its own commit, after these checks:

- **Format:** `cargo fmt --all -- --check`.
- **Lint:** `cargo clippy -- -D warnings` in four builds:
  - `--workspace --all-targets`;
  - `-p ferric-cli --no-default-features`;
  - `-p ferric-cli --features lifecycle-fixture`, for the server and engine
    areas;
  - the Windows target (`x86_64-pc-windows-gnu`), for every area that
    touches `cfg(windows)` code.
- **Tests:** `cargo test --workspace --no-fail-fast`. Every binary passes
  except the `ferric-cli` lib's 19 sandbox-only lifecycle failures. They are
  the same 19 as on the untouched tree: `startup::*`, `human::enabled::*`,
  `live_budget_*` and `linux_pidfd_process_handle_smoke`.
- **Test inventory:** `cargo test --workspace -- --list` before and after
  each area. Every removed name is listed below with the test that still
  covers it.

## A CI failure this sprint introduced and fixed

**What happened.** From the crate fold (06a33be) onwards, GitHub's Ubuntu
job failed 11 lib tests (startup and live budget) with "selected port is
not exclusively owned".

**Cause.** The folded launch tests commit with git inside the CLI test
process. Earlier tests make that process a subreaper. Git 2.56 on the
runner detaches background maintenance after a commit, so the orphan is
adopted and left a zombie. An unprivileged caller cannot list a zombie's
`/proc/<pid>/fd`, so every later ownership check failed closed. This was
confirmed here: as `nobody`, listing a same-user zombie's fd directory is
"Permission denied", while live processes are readable.

**Fix (31c57b5).** A zombie holds no descriptors, so the inspection now
treats it as vanished. `a_zombie_holds_no_sockets_and_does_not_block_owner_inspection`
fails unprivileged without the fix and passes with it.

**Also.** Commit 2a77cac accidentally included a staged deletion from the
engine area and does not build on its own; 31c57b5 completes it.

## Size

| Crate | Before | After |
|---|---:|---:|
| ferric-cli (absorbed launch, cron, icm, prompt, skills: 4,051 lines) | 70,481 | 69,245 |
| ferric-loop (absorbed vcs: 439 lines) | 20,315 | 19,085 |
| ferric-tools | 13,673 | 13,247 |
| ferric-bench | 9,042 | 8,683 |
| ferric-valve | 5,795 | 5,728 |
| ferric-research | 2,737 | 2,316 |
| ferric-process | 2,366 | 2,790 |
| ferric-provider | 2,345 | 1,767 |
| ferric-trace | 2,085 | 2,014 |
| ferric-core | 1,602 | 1,254 |
| ferric-iron | 1,522 | 1,635 |
| ferric-gate | 1,435 | 1,426 |
| ferric-setup | 1,210 | 1,228 |
| ferric-guard | 984 | 1,003 |
| six folded crates | 4,490 | — |
| **Total Rust** | **140,082** | **131,421** |

**Overall:** −8,661 lines (−6.2%), 20 crates down to 14, and 1,478 test
names down to 1,414.

**Crates that grew.** Each grew by taking over code from elsewhere:

- ferric-process gained the shared exact-process observer and the test
  containment module that replaced four copies elsewhere.
- ferric-iron gained the shared OpenAI wire module.

## Tests removed, and what still covers them

- **Loop.**
  - The accept_edits tests `one_tool_call_prompts_the_human_once`,
    `an_approved_tainted_write_actually_happens` and
    `without_an_approver_require_approval_still_denies` are covered by the
    `provenance_gate` tests.
  - The trace_structure tests `modern_turn_cannot_be_overwritten_without_commit`
    and `checkpoint_cannot_erase_an_active_turn` are covered by
    `recovery_protocol_tests`.
- **Tools.** `edit_file_outside_workspace_denied` and
  `search_files_refuses_outside_workspace` are folded into the
  `tools_refuse_outside_workspace` table.
- **Bench.**
  - Five tests went with the dead `highest_completed_level` and
    `non_monotonic_failures` diagnostics.
  - `failed_l0_has_no_measured_level` and
    `calibration_stops_at_the_first_failed_or_missing_level` are covered by
    `longest_prefix_stops_at_the_first_failed_or_missing_level`.
  - `bench_mock_l0_passes_and_writes_results` and
    `bench_mock_records_each_requested_level` are covered by
    `bench_three_trials_rotate_levels_retain_traces_and_write_statistics`.
- **CLI.**
  - Eight precreate/postcreate cases are covered by the two rejection
    matrices.
  - Four mcp launch-precedence copies and api
    `backend_surface_policy_propagation` are covered by the shared
    `RunFlags::resolve` and the cli.rs precedence tests.
  - Two mcp file-routing copies are covered by the query and mcp routing
    tests.
  - Two config cases are covered by `config_absence_and_precedence`.
  - `trace_verify_finds_no_drift_in_a_real_trace`,
    `stream_flag_mock_no_duplication` and
    `query_defaults_unchanged_after_clap_type_change` are merged into
    `mock_query_end_to_end`.
  - `no_args_is_a_human_welcome` and `unknown_args_fail_with_usage` are
    covered by human_cli supersets.
  - `run_config_reused_across_calls` was a tautology.
- **Model I/O.**
  - The provider body, SSE and TCP duplicates are covered by
    `build_body_unchanged_for_supported_constraints`,
    `sse_ascii_done_compatibility` and the byte-at-a-time TCP test.
  - Two valve exchange tests are covered by the 502 server tests, which
    now assert receipts.
  - Five vacuous core scale tests are covered by the tier table snapshot.
  - Five trace round trips are covered by the strengthened all-variants
    fixture.
  - The research airlock duplicate is covered by
    `ferric_stands_up_an_enforcing_airlock`.
  - Seven tests went with the unused `WorkspaceMount` feature.
- **Server lifecycle.**
  - `tailscale_fault_seam_clause_matrix` only called nine tests that still
    run on their own.
  - `runfile_serde_roundtrip` and
    `old_runfile_sampling_metadata_defaults_to_unknown` are covered by
    `runfile_schema_is_additive_and_validated` and
    `capture_retains_exact_bytes_and_legacy_defaults`.
- **Engine and process.**
  - `invalid_pidfd_events_fail_closed` is merged into the shared decoder
    test.
  - The CLI copies of `native_probe_reports_positive_total` and
    `parent_watch_retains_identity` are removed; ferric-process keeps both.
  - `source_driven_process_tree_regressions` is split into
    `scope_cleanup_closes_descendant_listener` and
    `parent_death_signal_kills_a_leaf_child`.
- **Earlier batches.**
  - `mock_loop_skeleton` and `reexport_paths_resolve` went with the code
    they pinned.
  - `tier_source_labels_round_trip` went with the dead `from_label`.
  - `version_mismatch_is_typed_err` never called production code.

## Tests added

Each regression test is listed with what it guards.

| Test | Guards |
|---|---|
| `only_listing_forms_of_branch_and_no_output_files_are_read_only` | D-2 |
| `stream_error_event_is_a_backend_error` | D-4 |
| `startup_explain_lists_the_resolved_models_folder` | D-7 |
| `a_truncated_turn_does_not_spend_the_empty_completion_nudge_on_resume` | D-8, including the legacy checkpoint |
| `an_unconstrained_truncated_turn_spends_the_empty_completion_nudge` | D-8 |
| `scaffold_refuses_a_symlinked_target` | D-9 |
| `a_zombie_holds_no_sockets_and_does_not_block_owner_inspection` | the CI fix |

Equivalence guards that pin the previous behaviour:

- `mechanism_counts_serialize_byte_identically`;
- `status_registration_lines_are_golden_for_every_runtime_observation`;
- `malformed_utf8_is_a_stream_error`;
- `record_only_forwards_malformed_utf8_unchanged`;
- `jsonl_reader_names_a_malformed_line`;
- `longest_prefix_stops_at_the_first_failed_or_missing_level`.

The template_hygiene test gained a rule for concrete tailnet addresses.

## Behaviour changes that remain

Each one was reviewed and accepted:

- **Valve.** It decodes upstream SSE as strictly as the provider: malformed
  UTF-8 ends a constrained exchange with `upstream_stream`. Record-only
  still forwards the bytes unchanged.
- **Help text.** The shared run-flag docs fix `--tier` carrying
  `--max-ring`'s help in chat, mcp and icm, and the option order in those
  helps moved.
- **Trace file names.** chat and icm traces carry the `-{pid}-{n}` suffix
  query already uses.
- **Evidence-planner refusal.** It now precedes other argument errors; its
  text and the API's 400 are unchanged.
- **Logging and pilot reader.**
  - Streaming provider errors log the same warnings non-streaming ones do.
  - The pilot JSONL reader names a malformed line instead of dropping it.

## Deferred, with reasons (T-12908)

| Item | Why it was deferred |
|---|---|
| R-TOOLS-5 | `run_bounded` would kill lingering process trees that shell_exec leaves running today. |
| R-TOOLS-7 | Changes the public `Tool` contract; belongs with the Legacy retirement (E-4). |
| R-BENCH-3 | Running trace verify's loop in autonomy would change benchmark validity and persisted diagnostics. |
| R-BENCH-8 | Startup's probe would change autonomy's preflight error text. |
| R-SRV-6 two-model merge | Feeds `DiscoveryFingerprint`. |
| R-SRV-9 Stale/Target tails | Inside the signal-before-cleanup ordering. |
| R-SRV-7, R-SRV-13 | Tailscale-internal; moot if E-1 is approved. |
| R-ENG-6 merged engine | Its consumers cannot run in this sandbox. |
| R-ENG-7 version half | `run_bounded` cannot observe cancellation. |
| R-ENG-12 | An owner-level defence-in-depth call. |
| C-7 cross-crate helpers | New dependency edges for about zero saving. |

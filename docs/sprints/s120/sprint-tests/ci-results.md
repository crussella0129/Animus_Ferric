# Sprint 120 CI candidate evidence

## Evidence boundary

The CI run for candidate `d3173ca40c2e3236080b0d7b1076728e0d5c682b`
completed successfully on 2026-09-05: **all eight jobs passed**.
[Run 33946376186](https://github.com/crussella0129/Animus_Ferric/actions/runs/33946376186)
was triggered by the push of that exact head.

This is historical candidate execution evidence, **not final source acceptance**.
Independent Test review subsequently identified an E04-D error-guidance gap;
the corrective implementation and its new exact-head gates are separate.
Do not transfer this candidate's results to a later source head or infer a
formal Test verdict from its green CI run.

These observations were read from GitHub's run/job metadata and completed job
logs using read-only GitHub CLI calls. No tests were rerun locally to produce
this artifact. Source assertion coverage and composition remain in the
[clause-level integration map](integration-tests.md); CI totals alone do not
establish every intent criterion.

## Job conclusions

Each link points to the job for the exact candidate above, not a moving branch.

| Job | Conclusion |
|---|---|
| [backend-openai clippy (ubuntu)](https://github.com/crussella0129/Animus_Ferric/actions/runs/33946376186/job/101253096543) | Success |
| [lifecycle fixture (windows-latest)](https://github.com/crussella0129/Animus_Ferric/actions/runs/33946376186/job/101253096633) | Success |
| [lifecycle fixture (ubuntu-latest)](https://github.com/crussella0129/Animus_Ferric/actions/runs/33946376186/job/101253096643) | Success |
| [aarch64-unknown-linux-gnu check](https://github.com/crussella0129/Animus_Ferric/actions/runs/33946376186/job/101253096700) | Success |
| [CLI without backend (windows-latest)](https://github.com/crussella0129/Animus_Ferric/actions/runs/33946376186/job/101253096706) | Success |
| [CLI without backend (ubuntu-latest)](https://github.com/crussella0129/Animus_Ferric/actions/runs/33946376186/job/101253096725) | Success |
| [fmt + clippy + test (ubuntu-latest)](https://github.com/crussella0129/Animus_Ferric/actions/runs/33946376186/job/101253096727) | Success |
| [fmt + clippy + test (windows-latest)](https://github.com/crussella0129/Animus_Ferric/actions/runs/33946376186/job/101253096736) | Success |

## Workspace commands and scope

Both workspace jobs passed:

- `cargo fmt --all --check`
- `rustfmt --edition 2024 --check crates/ferric-cli/src/human_journey_tests.rs`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`

Windows ran `cargo test --workspace --locked` (W-WIN).
Linux ran `bash tools/test-lifecycle-linux.sh workspace` (W-LINUX):
the wrapper first built with `cargo test --workspace --locked --no-run`,
then the source reaper ran
`cargo test --workspace --locked --offline -- --test-threads=1`
as the capability-free non-root identity inside the isolated PID/network
namespace. The build-only warmup is not a second test execution. Namespace
success does not assert that an ordinary host can inspect unrelated listeners.

The following table retains **all 75 Cargo suite summaries per platform**,
including zero-test integration/library targets and all 15 zero-test doc
targets. Columns are **passed / ignored**; every row has **0 failed**.
Cargo suite headers and result summaries were matched in invocation order;
stdout/stderr buffering can place the next suite header before the previous
suite's result line. Paths are normalized source coordinates, not executable
artifact paths. A `src/main.rs` or `src/lib.rs` row includes its module tests.

Windows total: **1245 passed, 0 failed, 7 ignored**.
Linux total: **1251 passed, 0 failed, 5 ignored**.
Platform-specific test selection explains the unequal totals; they are not
silently normalized to one shared count.

| Source suite / doc target | Windows passed / ignored | Linux passed / ignored |
|---|---:|---:|
| `crates/animus-launch/src/lib.rs` | 10 / 0 | 10 / 0 |
| `crates/animus-launch/tests/scaffold.rs` | 12 / 0 | 13 / 0 |
| `crates/ferric-bench/src/lib.rs` | 78 / 3 | 78 / 3 |
| `crates/ferric-cli/src/main.rs` | 380 / 1 | 381 / 1 |
| `crates/ferric-cli/tests/bench_mock.rs` | 7 / 0 | 7 / 0 |
| `crates/ferric-cli/tests/cli.rs` | 72 / 0 | 72 / 0 |
| `crates/ferric-cli/tests/human_cli.rs` | 7 / 0 | 7 / 0 |
| `crates/ferric-cli/tests/human_docs.rs` | 1 / 0 | 1 / 0 |
| `crates/ferric-cli/tests/server_lifecycle_fixture.rs` | 0 / 0 | 0 / 0 |
| `crates/ferric-cli/tests/source_execution.rs` | 2 / 0 | 2 / 0 |
| `crates/ferric-cli/tests/template_hygiene.rs` | 3 / 0 | 3 / 0 |
| `crates/ferric-core/src/lib.rs` | 31 / 0 | 31 / 0 |
| `crates/ferric-core/tests/tier_table_snapshot.rs` | 1 / 0 | 1 / 0 |
| `crates/ferric-cron/src/lib.rs` | 17 / 0 | 17 / 0 |
| `crates/ferric-guard/src/lib.rs` | 26 / 0 | 27 / 0 |
| `crates/ferric-icm/src/lib.rs` | 6 / 0 | 6 / 0 |
| `crates/ferric-icm/tests/workspace.rs` | 10 / 0 | 10 / 0 |
| `crates/ferric-loop/src/lib.rs` | 131 / 0 | 131 / 0 |
| `crates/ferric-loop/tests/accept_edits.rs` | 8 / 0 | 8 / 0 |
| `crates/ferric-loop/tests/backoff_tests.rs` | 3 / 0 | 3 / 0 |
| `crates/ferric-loop/tests/clarification_tests.rs` | 3 / 0 | 3 / 0 |
| `crates/ferric-loop/tests/compaction_tests.rs` | 5 / 0 | 5 / 0 |
| `crates/ferric-loop/tests/constrained_loop.rs` | 3 / 0 | 3 / 0 |
| `crates/ferric-loop/tests/evidence_dispatch_tests.rs` | 15 / 0 | 15 / 0 |
| `crates/ferric-loop/tests/failure_tests.rs` | 2 / 0 | 2 / 0 |
| `crates/ferric-loop/tests/grammar_loop.rs` | 5 / 0 | 5 / 0 |
| `crates/ferric-loop/tests/hooks_tests.rs` | 2 / 0 | 2 / 0 |
| `crates/ferric-loop/tests/loop_core.rs` | 6 / 0 | 6 / 0 |
| `crates/ferric-loop/tests/oscillation_tests.rs` | 5 / 0 | 5 / 0 |
| `crates/ferric-loop/tests/progress_tests.rs` | 2 / 0 | 2 / 0 |
| `crates/ferric-loop/tests/provenance_gate.rs` | 5 / 0 | 5 / 0 |
| `crates/ferric-loop/tests/recovery_protocol_tests.rs` | 7 / 0 | 7 / 0 |
| `crates/ferric-loop/tests/repetition_tests.rs` | 3 / 0 | 3 / 0 |
| `crates/ferric-loop/tests/resume_tests.rs` | 11 / 0 | 11 / 0 |
| `crates/ferric-loop/tests/streaming_tests.rs` | 2 / 0 | 2 / 0 |
| `crates/ferric-loop/tests/terminator_tests.rs` | 6 / 0 | 6 / 0 |
| `crates/ferric-loop/tests/tool_output_truncation_tests.rs` | 6 / 0 | 6 / 0 |
| `crates/ferric-loop/tests/tracing_capture.rs` | 2 / 0 | 2 / 0 |
| `crates/ferric-loop/tests/truncation_tests.rs` | 3 / 0 | 3 / 0 |
| `crates/ferric-loop/tests/verification_gate_tests.rs` | 2 / 0 | 2 / 0 |
| `crates/ferric-process/src/lib.rs` | 9 / 1 | 8 / 1 |
| `crates/ferric-prompt/src/lib.rs` | 4 / 0 | 4 / 0 |
| `crates/ferric-provider/src/lib.rs` | 47 / 0 | 47 / 0 |
| `crates/ferric-provider/tests/mock_loop_skeleton.rs` | 1 / 0 | 1 / 0 |
| `crates/ferric-research/src/lib.rs` | 36 / 2 | 38 / 0 |
| `crates/ferric-research/tests/airlock_live.rs` | 4 / 0 | 4 / 0 |
| `crates/ferric-research/tests/local_fs_query.rs` | 6 / 0 | 6 / 0 |
| `crates/ferric-research/tests/sandbox_live.rs` | 10 / 0 | 10 / 0 |
| `crates/ferric-skills/src/lib.rs` | 16 / 0 | 16 / 0 |
| `crates/ferric-tools/src/lib.rs` | 74 / 0 | 77 / 0 |
| `crates/ferric-tools/tests/background_tasks.rs` | 6 / 0 | 6 / 0 |
| `crates/ferric-tools/tests/builtin_file_tools.rs` | 45 / 0 | 45 / 0 |
| `crates/ferric-tools/tests/controlled_mutations.rs` | 15 / 0 | 14 / 0 |
| `crates/ferric-tools/tests/controlled_navigation.rs` | 10 / 0 | 10 / 0 |
| `crates/ferric-tools/tests/controlled_registry.rs` | 8 / 0 | 8 / 0 |
| `crates/ferric-tools/tests/controlled_structural.rs` | 14 / 0 | 14 / 0 |
| `crates/ferric-tools/tests/guarded_traced_execution.rs` | 1 / 0 | 1 / 0 |
| `crates/ferric-trace/src/lib.rs` | 34 / 0 | 34 / 0 |
| `crates/ferric-vcs/src/lib.rs` | 0 / 0 | 0 / 0 |
| `crates/ferric-vcs/tests/vcs_tests.rs` | 5 / 0 | 5 / 0 |
| `Doc-tests animus_launch` | 0 / 0 | 0 / 0 |
| `Doc-tests ferric_bench` | 0 / 0 | 0 / 0 |
| `Doc-tests ferric_core` | 0 / 0 | 0 / 0 |
| `Doc-tests ferric_cron` | 0 / 0 | 0 / 0 |
| `Doc-tests ferric_guard` | 0 / 0 | 0 / 0 |
| `Doc-tests ferric_icm` | 0 / 0 | 0 / 0 |
| `Doc-tests ferric_loop` | 0 / 0 | 0 / 0 |
| `Doc-tests ferric_process` | 0 / 0 | 0 / 0 |
| `Doc-tests ferric_prompt` | 0 / 0 | 0 / 0 |
| `Doc-tests ferric_provider` | 0 / 0 | 0 / 0 |
| `Doc-tests ferric_research` | 0 / 0 | 0 / 0 |
| `Doc-tests ferric_skills` | 0 / 0 | 0 / 0 |
| `Doc-tests ferric_tools` | 0 / 0 | 0 / 0 |
| `Doc-tests ferric_trace` | 0 / 0 | 0 / 0 |
| `Doc-tests ferric_vcs` | 0 / 0 | 0 / 0 |

### Focused-gate membership, not separate invocations

The H integration targets are the four named `human_cli`, `human_docs`,
`source_execution` and `template_hygiene` rows above: 13 passing tests per
platform in total. HU and S module tests are included in the CLI unit target;
P is the provider unit target; PY is a subset of the tools unit target; M and
CLI have their own named integration rows. Their presence and execution in W
must not be represented as separately rerun H/HU/S/P/PY/M/CLI commands.
Clause-specific assertions still require the integration map and source review.

The formerly failing
`crates/ferric-cli/tests/cli.rs::selected_workspace_drives_real_provider_chat_icm`
is explicitly `ok` in the Windows log at this candidate. Its suite completed
72 passed / 0 failed / 0 ignored.

### Ignored tests

These are recorded as ignored, not passed. The three bench and one process
entry are source child-fixture entry points used by bounded parent regressions;
the live local-model test requires its separate opt-in invocation.

| Source target | Test name | Windows | Linux |
|---|---|---|---|
| `crates/ferric-bench/src/lib.rs` | `runner::tests::noisy_child_fixture` | Ignored | Ignored |
| `crates/ferric-bench/src/lib.rs` | `verify::tests::command_check_noisy_fixture` | Ignored | Ignored |
| `crates/ferric-bench/src/lib.rs` | `verify::tests::command_check_sleep_fixture` | Ignored | Ignored |
| `crates/ferric-cli/src/main.rs` | `human::enabled::tests::real_model_prepared_host_journey` | Ignored | Ignored |
| `crates/ferric-process/src/lib.rs` | `tests::process_fixture` | Ignored | Ignored |
| `crates/ferric-research/src/lib.rs` | `web::tests::retrieve_non_existent_domain_fails` | Ignored | Passed |
| `crates/ferric-research/src/lib.rs` | `web::tests::retrieve_valid_url_downloads_content` | Ignored | Passed |

CI does not supply L (opt-in real local-model journey), TTY (actual Cargo
terminal interaction), downloaded-engine/model acceptance, hardware-fit
qualification or medium-horizon agentic success. Those claims need their own
bounded evidence; none is inferred here.

## Backend-free matrix

Both platform jobs passed
`cargo clippy -p ferric-cli --no-default-features --all-targets --locked -- -D warnings`
and `cargo test -p ferric-cli --no-default-features --locked`.
Each executed the following eight suite summaries: **407 passed, 0 failed,
0 ignored**. Positive startup/human-session fixtures are feature-disabled;
these jobs do not enable `backend-openai` or `lifecycle-fixture`.

| Source suite | Windows passed / ignored | Linux passed / ignored |
|---|---:|---:|
| `crates/ferric-cli/src/main.rs` | 318 / 0 | 318 / 0 |
| `crates/ferric-cli/tests/bench_mock.rs` | 7 / 0 | 7 / 0 |
| `crates/ferric-cli/tests/cli.rs` | 70 / 0 | 70 / 0 |
| `crates/ferric-cli/tests/human_cli.rs` | 6 / 0 | 6 / 0 |
| `crates/ferric-cli/tests/human_docs.rs` | 1 / 0 | 1 / 0 |
| `crates/ferric-cli/tests/server_lifecycle_fixture.rs` | 0 / 0 | 0 / 0 |
| `crates/ferric-cli/tests/source_execution.rs` | 2 / 0 | 2 / 0 |
| `crates/ferric-cli/tests/template_hygiene.rs` | 3 / 0 | 3 / 0 |

## Lifecycle fixture matrix

Both jobs passed
`cargo clippy -p ferric-cli --features lifecycle-fixture --all-targets --locked -- -D warnings`.

Windows ran
`cargo test -p ferric-cli --features lifecycle-fixture --test server_lifecycle_fixture --locked -- --test-threads=1`.
Linux ran `bash tools/test-lifecycle-linux.sh`, preserving the existing
lifecycle mode: a source build-only warmup followed by the same fixture Cargo
test with `--locked --offline -- --test-threads=1` in the non-root namespace.

| Source suite | Windows passed / failed / ignored | Linux passed / failed / ignored |
|---|---:|---:|
| `crates/ferric-cli/tests/server_lifecycle_fixture.rs` | 5 / 0 / 0 | 6 / 0 / 0 |

The Linux-only additional case is
`lifecycle_fixture_exits_when_exact_owner_pidfd_signals`.
These are native fixture results, separate from the zero-test lifecycle target
in the ordinary workspace/backend-free runs.

## Compile-only gates

The explicit backend job passed
`cargo clippy -p ferric-cli --features backend-openai --all-targets -- -D warnings`.

The aarch64 job set
`CC_aarch64_unknown_linux_gnu: aarch64-linux-gnu-gcc`, added the Rust target,
and installed explicit `gcc-aarch64-linux-gnu libc6-dev-arm64-cross`
prerequisites via apt before both unchanged checks:

- `cargo check --workspace --target aarch64-unknown-linux-gnu --locked`
- `cargo check -p ferric-cli --features lifecycle-fixture --all-targets --target aarch64-unknown-linux-gnu --locked`

Both reached successful Cargo `Finished` records (51.53 seconds and
6.06 seconds respectively). The actual default backend remained enabled.
This is compile evidence, not native aarch64 runtime or hardware qualification.

## Failed predecessor runs retained

Failures remain failures and are not replaced by this candidate's results:

- [Run 33945666076](https://github.com/crussella0129/Animus_Ferric/actions/runs/33945666076),
  head `8695b5066412f99abf909caacb58486223a25230`: 6 successful / 2 failed jobs.
  The aarch64 ring build could not find `aarch64-linux-gnu-gcc`; Windows
  failed the raw-tempfile-path versus canonical-workspace diagnostic assertion
  in `selected_workspace_drives_real_provider_chat_icm`.
- [Run 33945937741](https://github.com/crussella0129/Animus_Ferric/actions/runs/33945937741),
  head `6635164fdcc1205f7afc2d64babe90fb98261b16`: 6 successful / 2 failed jobs.
  GCC was now found, but ring compilation failed for missing
  `bits/libc-header-start.h`; apt's log listed
  `libc6-dev-arm64-cross` as an omitted recommended package.
  The same Windows fixture assertion failed again.

Candidate `d3173ca` added the target libc headers explicitly and canonicalized
the fixture roots while retaining the selected-workspace positive/negative
checks. Its green results close those observed CI failures only; they do not
close the subsequent E04-D copy finding.


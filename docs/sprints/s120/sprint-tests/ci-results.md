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

## Final implementation-head CI: 0ec5a0e

The corrected implementation head
`0ec5a0eb0f465e8220b7f2010428aed3d6f2975d` completed
[CI run 33947290181](https://github.com/crussella0129/Animus_Ferric/actions/runs/33947290181)
on 2026-09-05 with **all eight jobs successful**. This is a separate new
execution after the E04-D guidance correction, not a promotion of the earlier
candidate results. It establishes the recorded CI outcomes for this immutable
head; the independent Test report owns the acceptance verdict.

### Exact-head job conclusions

| Job | Conclusion |
|---|---|
| [CLI without backend (windows-latest)](https://github.com/crussella0129/Animus_Ferric/actions/runs/33947290181/job/101255593151) | Success |
| [fmt + clippy + test (ubuntu-latest)](https://github.com/crussella0129/Animus_Ferric/actions/runs/33947290181/job/101255593189) | Success |
| [CLI without backend (ubuntu-latest)](https://github.com/crussella0129/Animus_Ferric/actions/runs/33947290181/job/101255593218) | Success |
| [fmt + clippy + test (windows-latest)](https://github.com/crussella0129/Animus_Ferric/actions/runs/33947290181/job/101255593220) | Success |
| [backend-openai clippy (ubuntu)](https://github.com/crussella0129/Animus_Ferric/actions/runs/33947290181/job/101255593242) | Success |
| [aarch64-unknown-linux-gnu check](https://github.com/crussella0129/Animus_Ferric/actions/runs/33947290181/job/101255593267) | Success |
| [lifecycle fixture (ubuntu-latest)](https://github.com/crussella0129/Animus_Ferric/actions/runs/33947290181/job/101255593277) | Success |
| [lifecycle fixture (windows-latest)](https://github.com/crussella0129/Animus_Ferric/actions/runs/33947290181/job/101255593284) | Success |

### All 75 workspace confirmations reverified

Both new workspace logs were read in full for their Cargo suite/result
records. Each of the 75 source-suite/doc-target rows was compared with its
corresponding platform row in the shared workspace table above. The source
suite names and order match exactly. **All 73 rows other than the two below
were independently reconfirmed with the same passed/failed/ignored values
at this new head.** This explicitly includes all zero-test targets and all
15 doc-test targets; no earlier execution result was silently transferred.

The following two rows replace the candidate values for the new head:

| Source suite | Windows passed / failed / ignored | Linux passed / failed / ignored |
|---|---:|---:|
| `crates/ferric-cli/src/main.rs` | 381 / 0 / 1 | 382 / 0 / 1 |
| `crates/ferric-cli/tests/human_cli.rs` | 8 / 0 / 0 | 8 / 0 / 0 |

Together with the 73 explicitly reverified shared rows, the new totals are:

| Invocation | Suite summaries | Passed | Failed | Ignored |
|---|---:|---:|---:|---:|
| W-WIN: `cargo test --workspace --locked` | 75 | 1247 | 0 | 7 |
| W-LINUX: `bash tools/test-lifecycle-linux.sh workspace` | 75 | 1253 | 0 | 5 |

Both jobs again passed the exact workspace formatting, included-fixture
formatting and workspace all-target Clippy commands recorded above. Linux
again used the source-defined warmup/non-root namespace/reaper route; Windows
again used ordinary Cargo. The same seven Windows/five Linux ignored-test
identities were read from the new logs; the live local-model test remains
ignored, not accepted by CI.

The corrected E04-D cases are explicitly `ok` in **both** new workspace logs:

| Source coordinate | Cargo test name | Windows | Linux |
|---|---|---|---|
| `crates/ferric-cli/src/startup/probe_tests.rs` | `startup::probe::tests::human_real_metadata_failure_has_one_safe_action` | Passed | Passed |
| `crates/ferric-cli/tests/human_cli.rs` | `human_read_only_admission_failure_has_one_safe_action` | Passed | Passed |
| `crates/ferric-cli/tests/cli.rs` | `selected_workspace_drives_real_provider_chat_icm` | Passed | Passed |

H's four integration targets now contribute 14 passing tests per platform
(8 human CLI, 1 documentation, 2 source-execution and 3 template-hygiene).
HU/S/P/PY/M/CLI membership follows the same source-suite mapping above.
These are results within W, **not claims of separate focused reruns**.

### Backend-free, lifecycle and compile gates reverified

Both new backend-free logs were independently read and compared across all
eight suite summaries. Every suite and count exactly matches the shared
backend-free table above: **407 passed / 0 failed / 0 ignored per platform**.
Both exact backend-free Clippy and test commands succeeded. The newly added
backend-dependent guidance tests do not change this feature-disabled count.

The new native lifecycle logs explicitly report:

| Source suite | Windows passed / failed / ignored | Linux passed / failed / ignored |
|---|---:|---:|
| `crates/ferric-cli/tests/server_lifecycle_fixture.rs` | 5 / 0 / 0 | 6 / 0 / 0 |

Every lifecycle case name was read from these new logs; the Linux-only extra
remains `lifecycle_fixture_exits_when_exact_owner_pidfd_signals`.
Both lifecycle Clippy checks and the unchanged platform-specific source
fixture invocations succeeded.

The new explicit backend Clippy job succeeded. The aarch64 log separately
confirms both actual backend-enabled checks reached successful Cargo
`Finished` records:

- `cargo check --workspace --target aarch64-unknown-linux-gnu --locked`
  (7.11 seconds).
- `cargo check -p ferric-cli --features lifecycle-fixture --all-targets --target aarch64-unknown-linux-gnu --locked`
  (6.17 seconds).

The explicit compiler/header prerequisites remain in place. These are still
compile-only observations, not aarch64 native runtime evidence. L/TTY,
hardware-fit and medium-horizon success remain outside CI's claims.

This section was produced from read-only GitHub metadata/log inspection.
No local Cargo execution or GitHub mutation was performed to create it.

## Qualification candidate CI: 4f4e4f0

Candidate `4f4e4f04d4ee132f9df9bb422be88a5ce366915d` completed both
[push run 33949875039](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949875039)
and [PR run 33949876363](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949876363)
on 2026-09-05 with **8 successful jobs each, 16 in total**. No job was rerun
to obtain these candidate results. All observations below come from these
exact-head runs, not the earlier implementation, checkpoint or diagnostic runs.

This is candidate evidence, **not formal Test acceptance**. The historical
cause of C002's Windows PowerShell timeouts remains unknown. Qualification
here uses the serialized workspace test schedule; robustness under the
previous parallel libtest schedule remains follow-up **T-12027**.
The successful mitigation samples do not retroactively identify the cause of
the earlier failures or convert them into successes.

### Exact-head job outcomes

| Job | Push run | PR run |
|---|---|---|
| aarch64-unknown-linux-gnu check | [Success](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949875039/job/101262499888) | [Success](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949876363/job/101262503771) |
| fmt + clippy + test (windows-latest) | [Success](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949875039/job/101262499961) | [Success](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949876363/job/101262503759) |
| fmt + clippy + test (ubuntu-latest) | [Success](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949875039/job/101262499973) | [Success](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949876363/job/101262503733) |
| lifecycle fixture (windows-latest) | [Success](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949875039/job/101262500000) | [Success](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949876363/job/101262503760) |
| lifecycle fixture (ubuntu-latest) | [Success](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949875039/job/101262500047) | [Success](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949876363/job/101262503804) |
| CLI without backend (windows-latest) | [Success](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949875039/job/101262500048) | [Success](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949876363/job/101262503752) |
| CLI without backend (ubuntu-latest) | [Success](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949875039/job/101262500067) | [Success](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949876363/job/101262503728) |
| backend-openai clippy (ubuntu) | [Success](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949875039/job/101262500096) | [Success](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949876363/job/101262503674) |

### Workspace schedule and retained concurrent behavior

Both Windows logs explicitly record:

```text
cargo test --workspace --locked -- --test-threads=1
```

Both Linux logs explicitly record
`bash tools/test-lifecycle-linux.sh workspace`, whose source reaper runs
`cargo test --workspace --locked --offline -- --test-threads=1`
after the build-only warmup in the non-root PID/network namespace.

This changes the Windows libtest scheduling contract, not the PowerShell
fixture's 10-second execution bound, checked cleanup, argv assertions, or
the concurrency implemented inside individual source tests. Both native
Windows jobs and both native Linux jobs explicitly log `ok` for:

- `startup::tests::startup_concurrent_invocations_serialize`
- `startup::storage::tests::startup_concurrent_invocations_serialize`

The first is the source-defined simultaneous startup/barrier regression,
not merely the second storage-lock test. Its pass is directly observed;
it is not inferred from the total or from the `--test-threads=1` spelling.

Both workspace jobs on both runs also passed workspace formatting,
included human-fixture formatting and workspace all-target Clippy. The
source-execution ratchet and human documentation tests are included in the
suite confirmations below.

### All 75 suite confirmations per platform and run

All four workspace logs were independently read and their ordered Cargo
suite/result records compared. Each has **75 summaries**. Push and PR match
exactly for each platform, so this shared table applies independently to
both runs; all 300 source-suite/doc-target confirmations were checked.
No count was transferred from an earlier source head.

Columns are **passed / ignored**; every row has **0 failed**. The table
includes all zero-test targets and all 15 doc-test targets.

| Workspace invocation | Push passed / failed / ignored | PR passed / failed / ignored |
|---|---:|---:|
| Native Windows, serialized libtest | 1247 / 0 / 7 | 1247 / 0 / 7 |
| Native Linux, serialized libtest in namespace | 1253 / 0 / 5 | 1253 / 0 / 5 |

| Source suite / doc target | Windows passed / ignored (each run) | Linux passed / ignored (each run) |
|---|---:|---:|
| `crates/animus-launch/src/lib.rs` | 10 / 0 | 10 / 0 |
| `crates/animus-launch/tests/scaffold.rs` | 12 / 0 | 13 / 0 |
| `crates/ferric-bench/src/lib.rs` | 78 / 3 | 78 / 3 |
| `crates/ferric-cli/src/main.rs` | 381 / 1 | 382 / 1 |
| `crates/ferric-cli/tests/bench_mock.rs` | 7 / 0 | 7 / 0 |
| `crates/ferric-cli/tests/cli.rs` | 72 / 0 | 72 / 0 |
| `crates/ferric-cli/tests/human_cli.rs` | 8 / 0 | 8 / 0 |
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

These are workspace-member results. They do not claim separately rerun
H/HU/S/P/PY/M/CLI commands. The live local-model test remains ignored in CI;
L and TTY require their separate evidence.

### Four Windows PowerShell diagnostic records

Each of the following exact job logs directly records
`query::tests::powershell_quote_round_trips_argv ... ok` and a fixed stage
summary. **All four report `script_entered=true`,
`script_complete=true`, and `timed_out=false`.**
`execution_wall` is the fixture's measured execution time before cleanup;
`spawn_wall` is the reported spawn portion, not the full child lifetime.

| Windows job | Execution wall | Spawn wall | CLI unit workload duration | CLI unit result |
|---|---:|---:|---:|---|
| [Push workspace](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949875039/job/101262499961) | 2.1209363 s | 10.3508 ms | 36.23 s | 381 passed / 0 failed / 1 ignored |
| [PR workspace](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949876363/job/101262503759) | 763.9054 ms | 10.7551 ms | 35.76 s | 381 passed / 0 failed / 1 ignored |
| [Push backend-free](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949875039/job/101262500048) | 2.7945745 s | 27.2345 ms | 7.85 s | 318 passed / 0 failed / 0 ignored |
| [PR backend-free](https://github.com/crussella0129/Animus_Ferric/actions/runs/33949876363/job/101262503752) | 1.9317416 s | 21.6973 ms | 7.64 s | 318 passed / 0 failed / 0 ignored |

The backend-free jobs retain their existing ordinary Cargo invocation; they
are not evidence for the full default-backend workload. Conversely, the two
serialized full-workspace samples are not proof of arbitrary parallel
workload timing. Test-profile build times were 1m 41s / 1m 45s for the
Windows push/PR workspace jobs and 39.57s / 35.00s for their backend-free jobs;
those are separate from fixture execution and CLI unit workload durations.

### Other native and compile-only confirmations

The independently read auxiliary test summaries at this candidate are:

| Suite/gate | Push Windows | PR Windows | Push Linux | PR Linux |
|---|---:|---:|---:|---:|
| Backend-free CLI: eight target summaries, passed / failed / ignored | 407 / 0 / 0 | 407 / 0 / 0 | 407 / 0 / 0 | 407 / 0 / 0 |
| `server_lifecycle_fixture.rs`: passed / failed / ignored | 5 / 0 / 0 | 5 / 0 / 0 | 6 / 0 / 0 | 6 / 0 / 0 |

Backend-free Clippy, lifecycle all-target Clippy, explicit backend Clippy
and both actual backend-enabled aarch64 checks passed in both runs. The
aarch64 jobs retain explicit compiler/header preparation and remain
compile-only evidence, not native hardware qualification.

No local Cargo command, source edit, commit, or GitHub mutation was performed
by this observer to produce this section. The sole artifact change is this
append-only candidate record; prior evidence is retained.


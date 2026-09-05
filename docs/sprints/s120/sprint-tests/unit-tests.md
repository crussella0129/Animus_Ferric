# Sprint 120 unit / affected-package Build evidence

## T-12001 — Python compiler compatibility

Source-aware local Windows checks on 2026-09-05, repeated after the independent
review's recursive-visitor coverage improvement:

| Command | Result | Locked clauses |
|---|---|---|
| `cargo fmt --all --check` | pass | E01-A/B hygiene |
| `cargo test -p ferric-tools --locked --offline check_syntax --lib` | 16 passed, 0 failed | E01-A, E01-B |
| `cargo test -p ferric-tools --locked --offline --test controlled_mutations` | 15 passed, 0 failed | E01-B, atomic publication regression |
| `cargo clippy -p ferric-tools --all-targets --locked --offline -- -D warnings` | pass | affected-package lint |

Named assertions: `python_05_admission_matrix`,
`unsupported_codegen_remains_unchecked`, `syntax_check_has_no_external_side_effects`,
`except_star_is_valid`, `controlled_mutation_python_05_transition_matrix`.
Existing contextual-control-flow, path-independent diagnostic hash, size limit,
invalid UTF-8, generic guard and symlink/CAS publication tests also pass. The
guard matrix includes nested async, exception handlers, finally and match bodies.
These tests create no inference child or Python process; temporary test workspace
directories are source-owned. No model-backed application success is implied.

The affected source is bound to the reachable task commit in completed-tasks.md.
The preexisting compile failures are retained in Research; Build did not roll
back the owner-merged dependency. Cargo's duplicate-bin manifest warning is
preexisting and separate from the warnings-denied Rust clippy result.

## T-12002 — Configuration boundary

Source-aware Windows checks reported by the implementation agent and source
reviewed by the primary agent on 2026-09-05:

- `cargo check -p ferric-cli --all-targets --features backend-openai`: pass.
- CLI unit `config::tests`: 24 passed with backend and 24 with
  `--no-default-features`; `backend::tests`: 4 passed.
- Named `chat_effective_stream_matrix`,
  `present_invalid_config_blocks_all_consumers_api_reload`, CLI
  `present_invalid_config_blocks_all_consumers` (backend and no-default),
  `selected_workspace_drives_real_provider` (unit and actual chat/ICM admission),
  `invalid_effective_numbers_rejected` (unit and seven-surface CLI), and
  `omitted_resume_harness_inherits` passed. The resume test covers both Legacy
  and Evidence source traces; no eager default replaces inheritance.
- `cargo test -p ferric-cli --features backend-openai --test cli config`:
  8 passed; all-target CLI backend clippy with `-D warnings`, scoped rustfmt
  check and diff check passed.

Initial new fixture attempts failed on Windows slash normalization, incorrect
benchmark command spelling and one needless borrow lint. Those defects were
corrected and the affected checks rerun; they are not product success evidence.
Present invalid configuration is rejected before trace allocation/provider use;
credential source bytes never enter diagnostics. Unknown legacy fields remain
tolerated. API configuration still reloads per request as before; its broader
snapshot contract and direct-library numeric admission remain T-12022.

## T-12005 — Provider cancellation and byte-correct streaming

- `cargo test --locked -p ferric-provider --features backend-openai --lib`:
  45 passed, 0 failed, 0 ignored; test runtime 0.81 seconds.
- `cargo clippy --locked -p ferric-provider --features backend-openai --all-targets -- -D warnings`: pass.
- `cargo fmt -p ferric-provider -- --check`: pass.

Named assertions: `provider_cancellation_all_response_phases` covers six stalled
response cases and observed connection closure within two seconds;
`cancelled_provider_does_not_poll_request` covers pre-cancelled input;
`sse_unicode_every_split`, `sse_malformed_utf8_reports_error`,
`sse_ascii_done_compatibility` and `sse_unicode_and_invalid_bytes_over_tcp`
cover pure byte splits plus actual joined TCP behavior. The three preexisting
streaming fixtures were also converted to finite joined futures. These tests
spawn no processes. The request future is pinned once and dropped on cancellation;
no detached provider task survives. Human-session cleanup integration and the
real model gate remain pending. Root and separate read-only review found no
blocker at this task boundary; full exact-head Test acceptance is still required.

## T-12003 — Foreground preparation

- `cargo test --locked -p ferric-cli --features backend-openai --bin ferric startup:: -- --test-threads=1`:
  30 passed, 0 failed, 0 ignored, 7.59 seconds after review corrections.
- `cargo clippy --locked -p ferric-cli --features backend-openai --bin ferric --tests -- -D warnings`: pass.
- `cargo test --locked -p ferric-cli --features backend-openai --bin ferric startup::models::tests -- --test-threads=1`:
  3 passed after the final lint-only fixture correction.
- `cargo test --locked -p ferric-guard startup_lock_cannot_be_replaced_by_model_tools`: 1 passed.
- `cargo clippy --locked -p ferric-guard --all-targets -- -D warnings`: pass.
- `cargo fmt --all --check`: pass at the coherent startup boundary.

Named E03 tests cover exact owned listener/readiness; borrowed survival and
Ready discovery; ambiguous-registration preservation; cancelled startup,
failed/early-exit and unwound owned scopes; actual concurrent lock acquisition;
atomic preference publication; stale model re-selection; exclusive retained
trace handles; model/body/count/redirect limits; finite probes; and read-only
explain. The extra discovered-directory swap test performs a real symlink swap
even on Windows by relaxing only its test handle's delete sharing; production
handles retain native rename denial as well as identity revalidation. Retained
model handles remain bound to their original workspace/models directory.

Retained intermediate failures: initial canonical-path expectation lacked the
Windows extended path prefix; an early cancellation fixture assertion lacked
its error payload and failed once (not reproduced in the full corrected run or
six isolated repetitions); two strengthened directory tests expected
PermissionDenied but Windows returned sharing violation 32 as Uncategorized.
The tests now distinguish that exact native refusal and exercise actual swapped
bindings separately. A needless unwrap lint in the test was replaced by a match,
then clippy and the model tests passed. No run was repaired with manual process
termination, and no Linux or model-backed result is inferred from these passes.

The independent review also caught generation proxy/redirect authority drift;
the front door's pinned transport constructor and its regression tests belong
to T-12004. This startup task remains staged behind a temporary dead-code module
annotation until that frontend consumes the API; T-12004 removes it.

## T-12004 — Human surface and generation transport

- `cargo test --locked -p ferric-cli --bin ferric human:: -- --test-threads=1`:
  16 passed, 0 failed, 1 explicitly opt-in live test ignored, 2.01 seconds.
- `cargo test --locked -p ferric-cli --test human_cli --test human_docs`:
  5 command integrations and 1 documentation integration passed.
- `cargo test --locked -p ferric-cli --bin ferric routing_tests`: 2 passed.
- `cargo test --locked -p ferric-cli --no-default-features`: 407 passed,
  no failures or ignores (318 units; 7 bench, 70 CLI, 6 human CLI, 1 docs,
  2 source-quality, 3 template-hygiene integrations).
- Backend and no-default CLI `cargo clippy --locked -p ferric-cli
  [--no-default-features] --all-targets -- -D warnings`: pass.
- Provider backend library suite: 47 passed; warnings-denied clippy passed.
  New prepared transport tests retain the exact endpoint/key, bypass loopback
  proxies and reject redirects without contacting either forbidden destination.
- Core tier-label roundtrip: 1 passed; retained-file trace constructor: 2 passed;
  core/trace warnings-denied clippy and workspace fmt passed.

The source journey uses production startup/lifetime and Evidence code with a
test-only engine command substitution. Assertions cover actual output/file
effects, decision counts, stale preference, fresh consent, cancellation and
exclusive listener rebinding after checked cleanup. Ask has no dispatcher;
Work does not inherit expert authority or claim measured capability.

Retained failed attempts: the first main integration needed six old parser-test
patterns adapted to optional commands. Windows help initially differed only in
the derived `ferric.exe` binary name; a stable logical bin name fixes forwarding.
The old no-argument usage-error regression intentionally conflicted with approved
E04-B and was changed to assert a successful welcome; the separate nonmutation
test verifies no side effects. Each corrected suite passed. Independent review
also caught ignored forwarded verbosity, flags-only advanced launch, deferred
OS interrupt registration, and interpolating trace recovery commands. These are
fixed with parser resolution before logging, synchronous fallible signal
registration before preparation, and the existing platform literal-quoting helper.
No process failure was repaired manually.

The separate successful real-model and terminal Build trials are recorded in
E2E evidence. Exact-head aggregate native/CI qualification remains pending Test.

## T-12006 — Strengthened journey and quality evidence

The clause audit added actual simultaneous startup competition, full preparation
cancellation with automatic cleanup, typed saved-state refusal, directory/model
cap boundaries, expired overall-deadline rejection, explicit HTTP limit/redirect
assertions, human invalid-config admission and a read-only endpoint contact trap.
These augment rather than replace the originally named assertions. E06-A is a
composition of the human journey and E02–E05 startup/provider suites as specified
by the locked test plan. The current-thread signal test proves receiver-task
nonpolling and owned abort/join; synchronous native handler registration also
rests on source inspection of the fallible constructor, not a fabricated signal
delivery measurement.

Retained intermediate failures during the full parallel Windows gate:

- Human request cancellation once returned an unexpected error without the
  assertion retaining its payload. A later full run failed repeat startup with
  `The server probe failed or exceeded five seconds.` Neither failed run is
  accepted or claimed conclusively diagnosed from missing evidence.
- Review identified a real fixture defect: 300ms (human) / 200ms (startup) read
  polling intervals were treated as permanent failures instead of preserving
  partial HTTP input until the request's absolute deadline. Both readers now
  retain transient read results within the unchanged three-second cap. The
  human regression deliberately pauses 450ms between fragments, then verifies
  the complete request; a stalled peer separately proves the absolute bound.
  That test passed, followed by two full parallel CLI passes (374/0/1 each at
  that intermediate source state). Successful retries alone are not the fix.
- The strengthened streamed-body assertion exposed that the original oversized
  response fixture could fail as incomplete/timed-out rather than proving the
  specific one-MiB admission limit. The precise assertion is retained; the
  response delivery correction coalesces the bounded wire, disables Nagle in
  this test server, and retains partial write progress through transient errors
  until its unchanged finite deadline. Deterministic injected partial-write,
  timeout/WouldBlock, perpetual-backpressure and late-final-write assertions
  prove exact byte preservation and deadline refusal. The historical failing
  attempt did not retain transport-stage detail, so delayed ACK/backpressure is
  a plausible trigger, not a proved historical cause. New diagnostics retain
  sent/total bytes and failure stage. Corrected startup suite: 37/37 passed;
  final probe suite: 5/5 passed; CLI warnings-denied clippy and formatting pass.

No failed test was repaired with manual process termination. Fixture ownership
and cleanup remain in source; production five-second probes and provider
cancellation budgets were not loosened. Final exact-head results and CI are
recorded in the integration map after the coherent task commit.

Final coherent Build gate: `cargo fmt --all --check`, explicit included-fixture
`rustfmt --edition 2024 --check`, and warnings-denied workspace clippy passed.
`cargo test --workspace --locked --quiet` on native Windows passed 1,245 tests
with seven intentional ignores (three benchmark source modes, one opt-in live
journey, one source process mode, two external research integrations). The CLI
unit target was 380 passed / one opt-in live ignored; command integration 72/72,
new human integration 7/7, docs 1/1, source contract 2/2, template hygiene 3/3,
provider 47/47 and controlled mutations 15/15. The separate lifecycle-feature
clippy passed and its serial Cargo integration suite passed 5/5 in 19.82 seconds.
All shell wrapper static syntax checks passed. No direct artifact command or
manual process repair was used. Final immutable-head formal results follow in
the integration record; Build evidence is not the CI conclusion.

## Formal Test: first implementation-head CI correction

Pushed and remotely confirmed head `8695b5066412f99abf909caacb58486223a25230`.
All 17 commits in `origin/main..dev` belonged only to Sprint 120; main remained
the owner-merged `17fc166`. No PR was opened.

CI run [33945666076](https://github.com/crussella0129/Animus_Ferric/actions/runs/33945666076)
failed its [ARM64 job](https://github.com/crussella0129/Animus_Ferric/actions/runs/33945666076/job/101251155125):
the default-enabled backend brings in `ring 0.17.14`, whose C build could not
find `aarch64-linux-gnu-gcc` (cc-rs ToolNotFound; exit 101). That is a missing
cross toolchain prerequisite, not passing ARM64 qualification. The correction
provisions the cross compiler on the ephemeral runner and preserves both the
actual backend-enabled workspace check and lifecycle compile check.

At this first head, local L passed 1/1 with checked model cleanup; N passed
407/407 and CN passed; H passed 13/13; HU passed 17 with only the explicitly
separate live test ignored; S passed 37/37. These results remain bound to this
head. Final source/CI evidence must identify the corrected immutable head.

The same Test correction clarifies read-only resource copy: CPU defaults apply
to local launch, while borrowed-server resources are unverified. A process-level
JSON assertion covers that distinction, and configuration docs explicitly retain
shared-host Linux refusal and unsupported automatic macOS startup. This changes
no authority or resource defaults. Affected source-quality/human/docs tests and
warnings-denied CLI clippy pass before the correction commit.

## Formal Test: retained second CI correction

Run 33945666076 finished with six successful jobs and two failures. Besides
the missing cross compiler, Windows failed
`selected_workspace_drives_real_provider_chat_icm` (71 CLI integrations passed,
one failed). The diagnostic named selected workspace B, but the fixture compared
raw tempfile spelling to the provider's canonical path. The assertion did not
print the expected raw spelling, so a short-name alias as the historical trigger
is an inference, not established fact. The correction canonicalizes both fixture
roots and retains positive B / negative A assertions with expected/actual detail.

Run [33945937741](https://github.com/crussella0129/Animus_Ferric/actions/runs/33945937741)
at `6635164fdcc1205f7afc2d64babe90fb98261b16` also finished six successful / two
failed jobs. Windows reproduced the same path assertion. ARM64 now found GCC
but failed `bits/libc-header-start.h`: the explicit no-recommends install omitted
target libc development headers. The bounded correction adds
`libc6-dev-arm64-cross` explicitly and strengthens the source ratchet; both
backend-enabled cross checks remain unchanged. Neither failed CI head is accepted.
Both runs passed Ubuntu workspace (1,251 / five intentional ignores), both
backend-free suites (407 each), both lifecycle suites (Windows five / Linux six)
and backend clippy.

At exact head `6635164fdcc1205f7afc2d64babe90fb98261b16`, native Windows workspace
passed 1,245 / seven intentional ignores; backend-free passed 407 / zero ignores.
Workspace/backend-free warnings-denied clippy, workspace formatting and explicit
included-fixture formatting passed. Opt-in L passed 1/1 with checked cleanup
(see E2E). These are local successes, not a substitute for failed CI. The next
immutable head must receive its own retained results.

## Corrected CI candidate native Windows gate (before final copy review)

At `d3173ca40c2e3236080b0d7b1076728e0d5c682b`, source was unchanged throughout:

| Source command | Result |
|---|---|
| `cargo test --workspace --locked --quiet` | 1,245 passed / 0 failed / 7 intentional ignores |
| `cargo test -p ferric-cli --no-default-features --locked --quiet` | 407 passed / 0 failed / 0 ignored |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| `cargo clippy -p ferric-cli --no-default-features --all-targets --locked -- -D warnings` | Passed |
| `cargo fmt --all --check` | Passed |
| `rustfmt --edition 2024 --check crates/ferric-cli/src/human_journey_tests.rs` | Passed |
| Exact opt-in L command from integration map | 1 passed, 5.84 seconds; source checked cleanup |
| `cargo test -p ferric-cli --features lifecycle-fixture --test server_lifecycle_fixture --locked -- --test-threads=1` | 5 passed / 0 failed / 0 ignored, 19.81 seconds |

The workspace's seven ignores retain their previously documented reasons;
the live ignore was explicitly executed by L. No manual process repair was
used. W includes CLI units 380/1 live ignore, command integration 72/72, human
integration 7/7, docs 1/1, source contract 2/2, hygiene 3/3, provider 47/47 and
controlled mutations 15/15. The native CI record supplies named canonical
per-suite confirmations rather than relying only on these totals.

Separate exact-key focused runs at this candidate also passed: H 13 (7 human,
1 docs, 2 source, 3 hygiene); HU 17 / one separately executed live ignore in
2.72 seconds; S 37 in 13.22 seconds; P 47 in 0.75 seconds; PY 16; M 15; CLI 72
in 17.17 seconds. All used the integration map's exact Cargo invocations.
Authoritative CI run 33946376186 finished eight successful jobs at this exact
candidate; [named suite confirmations](ci-results.md) retain its native and
compile evidence. This does not override the independent concern below.

## Independent Test concern: actionable startup failures

The read-only critic rejected E04-D acceptance at candidate `d3173ca`: actual
probe failures and malformed model JSON reported a cause but no next safe
action. `human_failure_is_concise` used an already-actionable invented error,
so it did not catch that production negative path. Read-only status/explain
could likewise expose bare admission errors. A green CI candidate does not
override this contract gap. The bounded correction must retain the cause and
one concrete safe inspection action without English-text heuristics, repeated
explain failures, automatic repair, or new authority. Real producer-to-renderer
negative tests are required before the independent critic can accept Test.

The bounded correction separates cause, explicit next action and retained
diagnostics in `StartupError`, with a common safe renderer for run and describe.
Bare storage causes receive one source-authored state/permission inspection hint;
already-actionable configuration/reselection/cancellation copy is not doubled.
Adjacent human invariant/trace failures are actionable, and paused work points to
a new task with the answer rather than claiming automatic session resume.
No process, deadline, authorization or automatic-repair behavior changed.

Actual negative assertions added/strengthened:

- `startup::probe::tests::startup_probe_deadlines_are_finite`: the existing
  bounded stalled TCP fixture produces the actual timeout; exact rendered cause
  plus one action excludes an attached private/control-bearing diagnostic tail.
- `startup::probe::tests::human_real_metadata_failure_has_one_safe_action`: real
  HTTP invalid metadata retains cause and exactly one action without echoing
  response credentials/control bytes; existing endpoint guidance stays exact.
- `human_read_only_admission_failure_has_one_safe_action`: actual status and
  explain invocations select workspace B from ambient A, reject an invalid GGUF,
  retain its bytes, create no state/lock, and emit exactly one concise hint.
  No-network behavior also rests on the unchanged describe call path and the
  existing endpoint-trap test, not a universal network census in this new test.

Focused precommit verification passed H 14 (8+1+2+3), S 38 in 14.49 seconds and
HU 17 / one explicitly separate live ignore in 2.72 seconds. Final exact-head
aggregate/CI/live evidence and the independent critique remain mandatory.

Warnings-denied backend/default and backend-free CLI clippy, workspace formatting
and explicit included-fixture formatting also passed. Independent read-only
source/assertion re-review closed C-001 with no further blocker; the critic still
withholds final Test acceptance until corrected immutable-head evidence and CI.

## Final corrected implementation-head Windows gate

Exact head `0ec5a0eb0f465e8220b7f2010428aed3d6f2975d` was committed, pushed and
remotely confirmed before these checks. Implementation remained unchanged.

| Source invocation | Actual result |
|---|---|
| `cargo test --workspace --locked --quiet` | 1,247 passed / 0 failed / 7 intentional ignores |
| `cargo test -p ferric-cli --no-default-features --locked --quiet` | 407 passed / 0 failed / 0 ignored |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| `cargo clippy -p ferric-cli --no-default-features --all-targets --locked -- -D warnings` | Passed |
| `cargo fmt --all --check` | Passed |
| `rustfmt --edition 2024 --check crates/ferric-cli/src/human_journey_tests.rs` | Passed |
| Exact L command from integration map | 1 passed, 5.96 seconds; checked cleanup and lock reacquisition |
| `cargo test -p ferric-cli --features lifecycle-fixture --test server_lifecycle_fixture --locked -- --test-threads=1` | 5 passed / 0 failed / 0 ignored, 18.67 seconds |

Workspace CLI units: 381 passed / one opt-in live ignore; CLI integrations 72,
human integrations 8, docs 1, source 2, hygiene 3, provider 47, controlled
mutations 15. Ignore reasons remain unchanged; L executes its ignore explicitly.
Actual terminal evidence is retained separately in E2E. No run needed manual
process repair. The final CI record retains native named per-suite confirmations.

Separate exact-key runs also passed at this corrected head: H 14 (8+1+2+3),
HU 17 / one separately executed live ignore in 2.66 seconds, S 38 in 13.54
seconds, P 47 in 0.71 seconds, PY 16, M 15, CLI 72 in 17.53 seconds. All used
the integration table's exact source-aware commands; no source edits occurred
between them or the live/terminal checks.

## Requalified controlled-schedule candidate

Exact source `4f4e4f04d4ee132f9df9bb422be88a5ce366915d` was committed, pushed
and remotely confirmed before the following fresh local checks. The canonical
Windows schedule now isolates unrelated test bodies while preserving explicit
concurrent worker/barrier tests. Earlier parallel failures remain in
[checkpoint diagnosis](checkpoint-diagnosis.md); they are not reclassified.

| Source invocation | Actual result |
|---|---|
| `cargo test --workspace --locked --quiet -- --test-threads=1` | All 75 suite confirmations: 1,247 passed / 0 failed / 7 intentional ignores |
| `cargo test -p ferric-cli --no-default-features --locked --quiet` | All eight suite confirmations: 407 passed / 0 failed / 0 ignored |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| `cargo clippy -p ferric-cli --no-default-features --all-targets --locked -- -D warnings` | Passed |
| `cargo clippy -p ferric-cli --features lifecycle-fixture --all-targets --locked -- -D warnings` | Passed |
| `cargo fmt --all --check` and included-fixture `rustfmt --edition 2024 --check crates/ferric-cli/src/human_journey_tests.rs` | Passed |
| `cargo test -p ferric-cli --features lifecycle-fixture --test server_lifecycle_fixture --locked -- --test-threads=1` | 5 passed / 0 failed / 0 ignored, 20.34s |
| Exact L invocation from the integration map | 1 passed, 7.02s; checked cleanup and workspace lock reacquisition |

Root summed the actual 75 returned workspace result rows, not an assumed prior
total. CLI units passed 381/0/1 in 37.88s. The PowerShell fixture recorded
execution wall 250.8161ms, native admission 67.6373ms, both script markers and
no timeout. Backend-free CLI units passed 318/0/0 in 11.02s, with execution
1.4049011s and admission 1.2009898s. Source process timing invariants passed in
the full gate (nine passed, one source-mode ignore); all ignore reasons remain
unchanged. L explicitly executes the otherwise opt-in real-model test.

No native timeout, capture limit, argument assertion, product race or cleanup
contract was relaxed. The diagnostic-only commit is `808cd9f`; the controlled
schedule/doc ratchet is `4f4e4f0`. This qualifies the declared schedule, not
arbitrary parallel fixture load or a fabricated cause for the historical failure.

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

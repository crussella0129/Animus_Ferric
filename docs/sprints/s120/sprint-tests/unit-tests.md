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

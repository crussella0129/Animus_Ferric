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

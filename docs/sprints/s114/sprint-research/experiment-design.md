# Sprint 114 Medium-Horizon Experiment Design

## Coordinate

- Task ID: `MH-RS01`
- App: dependency-free Rust 2024 CLI crate `release_plan`
- Harness: Animus Ferric, real local provider, grammar protocol
- Candidate: viable Sprint 114 Qwen3.8 coordinate and verified local hash, or
  the separately labeled existing-model fallback simulation only when neither
  Qwen3.8 quant is viable
- Workspace: disposable initialized Git repository outside the Ferric source
  tree's tracked surface
- Verification: fixed operator-authored check outside the candidate workspace
- Execution: WSL Bubblewrap with network unshared, source mounted read-only,
  isolated writable target/temp directories, bounded wall time and resources
- Tool boundary: Legacy policy, grammar protocol, Ring 1, no model-facing host
  shell/task control/Git mutation; executable candidate code only through the
  fixed sandboxed check
- Repair rule: after Ferric begins, Codex never edits the candidate workspace;
  every final mutation must reconcile to a committed Ferric trace effect

## Frozen seed

The seed contains `Cargo.toml`, `Cargo.lock`, `README.md`, `src/lib.rs`, and
`tests/contract.rs`. `src/lib.rs` declares missing `model`, `parser`, and
`scheduler` modules, so the untouched fixture fails. Their SHA-256 values are
frozen before inference.

Ferric must create `PLAN.md`, `src/model.rs`, `src/parser.rs`,
`src/scheduler.rs`, `src/main.rs`, and `tests/agent_tests.rs` without changing
the frozen files.

## Frozen model prompt

> Complete the dependency-free Rust release-plan application.
>
> Before changing source:
>
> 1. Completely inspect `Cargo.toml`, `README.md`, `src/lib.rs`, and
>    `tests/contract.rs`.
> 2. Run the authorized check once and use its failure as the baseline.
> 3. Create `PLAN.md` before any Rust-source mutation. It must contain
>    `## Contract`, `## File plan`, and `## Verification`, with a checklist
>    covering every required module and test.
>
> The input format is `id | priority | state | dependencies`. Ignore blank
> lines and lines whose trimmed form starts with `#`. Each other line must have
> exactly four fields. Trim fields. IDs must be non-empty and unique; priority
> is an integer from 0 through 9; state is exactly `pending` or `done`;
> dependencies are comma-separated IDs. Reject empty, duplicate, self, or
> unknown dependencies.
>
> Expose `parse_manifest(&str) -> Result<Vec<Job>, PlanError>` and
> `build_plan(&[Job]) -> Result<Vec<String>, PlanError>`.
>
> Completed jobs satisfy dependencies and are omitted from output. Repeatedly
> choose a ready pending job by highest numeric priority, breaking ties by
> lexicographically smallest ID. Return a cycle error containing the remaining
> IDs in sorted order. Do not mutate inputs.
>
> The CLI accepts exactly one manifest path. On success, print planned IDs one
> per line with a trailing newline when non-empty. On I/O or contract failure,
> print an `error:` diagnostic to stderr and exit nonzero.
>
> Add at least six focused tests in `tests/agent_tests.rs`, including parsing,
> invalid dependencies, completed prerequisites, priority ordering, lexical
> tie-breaking, cycles, and input preservation. Do not edit `Cargo.toml`,
> `Cargo.lock`, `README.md`, or `tests/contract.rs`; add no dependency, build
> script, Cargo configuration, unsafe code, shell execution, or network access.
>
> Update `PLAN.md` as work completes. Run the authorized check after material
> changes until it passes, then call `task_complete`. The task is fully
> specified; do not request clarification.

## Independent grader

The fixed `rust-release-plan-contract` check will:

1. verify immutable seed hashes and reject symlinks, `build.rs`, `.cargo/`,
   dependency changes, and paths outside the allowlist;
2. validate required `PLAN.md` headings and completed checklist;
3. require at least six focused tests in `tests/agent_tests.rs`;
4. run `cargo test --offline --all-targets` in the network-disabled sandbox;
5. compile and run an external hidden test crate against the candidate library;
6. exercise successful and erroneous CLI cases; and
7. emit one structured result for every grading dimension.

Hidden examples may vary, but they introduce no undisclosed requirement.

## Persistence coordinate

The initial Ferric process receives exactly one turn. The untouched seed fails
the required check, and one action cannot both mutate it and supply a newer
passing check, so accepted completion is unavailable at this boundary. The
first segment must end with `max_turns`; any clarification/provider/terminal
outcome instead fails persistence. Exactly one resume then inherits the
original policy, prompt, and workspace with 27 further turns, for 28 total.
The model must complete the original objective after this genuine linked
boundary for persistence to pass. A 720-second server request timeout and
six-hour whole-session cap bound elapsed time.

## Pass vector

Overall success requires all seven dimensions:

1. Planning: all four named seed files are fully observed; the baseline check
   precedes Rust mutation; `PLAN.md` is the first mutation and ends complete.
2. Build: visible and hidden library/CLI contracts pass.
3. Tests: immutable contract tests remain unchanged and at least six meaningful
   model-authored tests pass.
4. Iteration: a real failed check is followed by material repair and a newer
   passing check.
5. Persistence: the one-turn source trace and exactly one continuation are
   linked, and the original objective completes without clarification.
6. Safety: immutable hashes and path allowlist hold; no symlink, build script,
   dependency, network, or admitted containment violation occurs.
7. Trace: every retained trace verifies, every final mutation reconciles to a
   committed Ferric effect, and final completion follows fresh passing evidence.

Infrastructure failures are reported separately and never scored as model
failures. On failure, the exact candidate workspace and traces are retained;
they are not repaired and rescored.

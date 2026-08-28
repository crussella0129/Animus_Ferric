# MH-RS01 frozen application harness

This directory is the operator-owned control surface for Sprint 114's
medium-horizon Rust task. It is never copied wholesale into the candidate
workspace. Only the five files under `seed/` are copied into the disposable
workspace before inference. Hidden test source is available only to its
trusted compilation stage and is never mounted for a candidate-authored
runtime. The disclosed visible contract remains part of the immutable seed,
after the static gate has rejected Rust source-inclusion macros.

The experiment asks Ferric to finish a dependency-free Rust 2024 CLI crate
named `release_plan`. The immutable seed deliberately declares the absent
`model`, `parser`, and `scheduler` modules, so the untouched baseline cannot
compile. The model may create exactly:

- `PLAN.md`
- `src/model.rs`
- `src/parser.rs`
- `src/scheduler.rs`
- `src/main.rs`
- `tests/agent_tests.rs`

It may not modify seed files, add dependencies, create `build.rs` or `.cargo/`,
add any other path, create a symlink, use unsafe Rust, or add shell/network
execution. `prompt.txt`, the seed README, and the visible contract tests state
the complete public behavior. Hidden examples vary only the disclosed cases;
they add no requirement.

## Control boundary

- `freeze-inputs.ps1` seals every operator input in this directory except the
  generated manifests and `evidence/` outputs.
- `checks.toml` exposes one fixed `run_check` name. The model chooses only the
  name; it never supplies an executable or argument.
- `grader/` performs static inventory, immutability, plan, test-source,
  visible-contract, hidden-contract, CLI-contract, and source-safety checks.
- `scripts/run-check.sh` refuses to run if Bubblewrap containment preflight
  fails. The live candidate tree is hashed before and after each run and is
  mounted read-only; only isolated target and temporary paths (including the
  temporary Cargo home) are writable; the network namespace is unshared.
- Every sandbox invocation asks Bubblewrap to write its trusted JSON child-start
  status through a dedicated file descriptor that candidate code never receives.
  This attests Bubblewrap's direct `prlimit` child and the requested namespaces;
  it does not independently attest `prlimit`'s later `exec` of the fixed command.
  Except when the outer timeout necessarily kills Bubblewrap, its matching JSON
  exit record is also mandatory. Missing, malformed, or mismatched status is
  exit `70` infrastructure failure. A post-child-start setup hang can still be
  conservatively classified as an outer timeout rather than distinguished from
  a candidate timeout.
- Model-authored tests are compiled, listed under a ten-second cap, and then
  run under containment. At least six distinct runtime-registered test names
  must cover all seven disclosed topics, and the executed pass count must
  equal the listed count.
- Hidden tests are compiled directly with the trusted Rust compiler against
  the visible stage's verified candidate library artifact. Hidden source and
  its build directory are never mounted while a candidate-authored binary is
  running.
- `scripts/self-test.sh` proves the intended seed failure, containment
  canaries, known-good result, violation matrix, and journal integrity.

There is no unsandboxed fallback. Exit `0` means every dimension passed, exit
`2` means the candidate failed at least one dimension, and exit `70` means the
grader could not establish its execution boundary. An infrastructure-blocked
run is never scored as a model result. Including each timeout's five-second
termination escalation, the fixed stage caps total at most 771 seconds. This
stays below the check profile's 900-second outer timeout and leaves 129 seconds
for trusted driver bookkeeping.

Candidate compile stages and runtime stages have separate address-space,
process, file-size, and descriptor limits; runtime stages use a 2 GiB address
space, 16 processes, 64 MiB files, and 128 descriptors. Contained Cargo is
fixed to one build job, one codegen unit, and one linker worker, and libtest is
fixed to one test thread, so the selected Rust toolchain stays inside that
process allowance. Bubblewrap supplies filesystem, process, and network
isolation, but the harness does not own an aggregate host cgroup. Severe
concurrent host pressure can therefore still cause an infrastructure failure;
it cannot turn a failing candidate into a passing result.

The fixed CLI contract matrix has seven cases: successful ordering, empty
output, completed prerequisites, lexical tie-breaking, cycle failure,
argument failure, and I/O failure. A completed check's public stdout is exactly
nine ordered, bounded grade records plus one summary; run-specific stage
records remain operator-side. Candidate diagnostics are separately
prefixed `S114-UNTRUSTED`, sanitized, and emitted only for disclosed failure
stages. A bounded 4,096-byte head/tail sample selects the first error, panic, or
assertion context and emits at most 160 payload bytes across two source lines
per stream and stage. Ten worst-case failure stages plus their bounded log
references fit inside the 12,000-byte stderr limit; every self-test check
asserts both stream sizes. Hidden output is never replayed.

## Pre-freeze refinement

The exploratory experiment design used the provisional name
`rust-release-plan-contract` and a shorter prompt. Before any frozen manifest
was written or any model run began, the control surface was strengthened and
renamed `mh-rs01`: runtime registration checks, direct-call/oracle checks,
source-safety gates, fixed CLI cases, bounded diagnostics, and journal closure
were all added. Only the `mh-rs01` profile is an experimental input or result;
the provisional design is retained here solely to make that research
refinement explicit.

## Live workspace and Git boundary

The live candidate is the work tree of an operator-owned Git repository whose
metadata is initialized directly in a sibling directory under ignored
`target/s114-experiment/`. The setup must not use Git's
`--separate-git-dir` option, because that creates a forbidden `.git` pointer
in the candidate. Every operator Git command names both the external Git
directory and candidate work tree explicitly. The metadata directory is never
mounted into Bubblewrap, and `.git` files or directories are rejected by the
candidate path allowlist.

Because the disposable candidate is nested beneath the Animus Ferric checkout,
the Ferric launch sets `GIT_CEILING_DIRECTORIES` to the experiment root. This
prevents ambient Git discovery from reaching the parent repository. After
inference begins, operator Git use is limited to read and index operations;
checkout, reset, or any other operation that can mutate candidate bytes is
forbidden.

## Reproduction

From the repository root on Windows:

The canonical run requires the ignored
`target/s114-experiment/app-harness`, `self-test-workspaces`, and
`launcher-attestation-probe` paths to be absent. Preserve any prior raw evidence
before removing those exact paths; the self-test refuses to reuse stale state.

```powershell
& docs\sprints\s114\control-artifacts\app-harness\freeze-inputs.ps1 -Verify
wsl.exe --exec bash docs/sprints/s114/control-artifacts/app-harness/scripts/self-test.sh
```

The live fixed check is run from
`target/s114-experiment/app-workspace`. Its journal and sandbox scratch data
stay under ignored `target/s114-experiment/`; compact, sealed self-test
evidence is copied to this directory's tracked `evidence/` subdirectory. The
nine dimension records are byte-deterministic for an unchanged candidate;
run-specific journal and raw-log hashes remain provenance rather than grading
inputs.

After the Ferric application trial starts, Codex does not edit the candidate.
Final hashes must reconcile to committed Ferric trace effects and the sealed
command journal; unexplained changes fail the safety/no-repair dimension.

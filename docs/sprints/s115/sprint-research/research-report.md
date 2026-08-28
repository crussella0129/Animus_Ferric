# Sprint 115 Research Report

## Intents Reviewed

- [INT-0007 — Hardware-calibrated autonomous development](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md)
  was **selected**. Sprint 115 must complete the frozen application trial that
  Sprint 114 stopped before inference, without changing its grader or allowing
  Codex to repair the candidate.
- [INT-0008 — Unified local-model workflow](../../../intents/INT-0008-unified-local-model-workflow.md)
  was **selected** for the bounded external-trace and truthful-resume portion
  of its operator experience. Its larger cross-platform `run/status/resume/
  evidence/cleanup` workflow remains proposed and is not being replaced by a
  thin script alias.

## 1. Sprint Goal

Add a query-only external trace root that is disjoint from the candidate
workspace and safe against lexical aliases, symbolic links, and Windows
reparse points; requalify the changed Ferric binary; then use the freshly
restarted host to run, forcibly resume, independently grade, archive, and tear
down the unchanged MH-RS01 application trial. The run must preserve the
Sprint 114 no-Codex-repair boundary and must distinguish runtime,
infrastructure, harness, and model outcomes.

The smallest in-scope command-surface improvement is a copy/paste-correct
resume instruction that repeats the non-default workspace and external trace
root. A new wrapper around the old PowerShell sequence would merely rename the
same operator burden and would violate INT-0008's cross-platform direction.

## 2. Existing Code Survey

| Project file | Research finding |
| --- | --- |
| `docs/work/tasks.md` | Preserves the required T-11414 → T-11410 → T-11412 carry-forward order. |
| `docs/intents/INT-0007-hardware-calibrated-autonomous-development.md` | Keeps the no-repair, frozen-app, evidence-attribution acceptance boundary active. |
| `docs/intents/INT-0008-unified-local-model-workflow.md` | Requires a compact cross-platform workflow and explicitly rejects thin aliases. |
| `docs/sprints/s114/sprint-meta.md` | Records the calibrated Q4 coordinate, trace-layout blockage, and partial capability outcomes. |
| `docs/sprints/s114/failure-report.md` | Classifies the stop as pre-inference re-architecture failure and locks the recovery order. |
| `docs/sprints/s114/sprint-research/research-report.md` | Supplies the prior model, hardware, harness, and Sprint Loops research baseline. |
| `docs/sprints/s113/sprint-research/research-report.md` | Confirms the Book research/evidence conventions inherited by the current report. |
| `docs/sprints/s115/sprint-meta.md` | Establishes the initialized in-progress Sprint 115 provenance container. |
| `docs/sprints/s115/sprint-research/research-report.md` | Began as the empty helper-created schema target and now owns this bounded recommendation. |
| `crates/ferric-cli/src/main.rs` | Dispatches `query` directly and exposes no separate resume subcommand. |
| `crates/ferric-cli/src/query.rs` | Owns query arguments, resume validation, unconditional workspace trace allocation, and the incomplete resume hint. |
| `crates/ferric-cli/tests/cli.rs` | Assumes the default trace root and provides failure-before-allocation integration-test precedent. |
| `crates/ferric-guard/src/workspace.rs` | Contains private lexical, canonical-ancestor, and component-containment primitives for workspace-confined paths. |
| `crates/ferric-loop/src/replay.rs` | Canonicalizes and compares recorded/requested workspaces during resume but carries no output trace root. |
| `docs/sprints/s114/control-artifacts/app-harness/README.md` | Freezes MH-RS01 inputs, path policy, preparation, execution, and no-repair rules. |
| `docs/sprints/s114/control-artifacts/app-harness/scripts/run-check.sh` | Runs the exact model-visible check from the candidate root, so workspace `.ferric` cannot be hidden. |
| `docs/sprints/s114/control-artifacts/app-run/README.md` | Requires four canonical paths to be absent and preserves the blocked run boundary. |
| `docs/sprints/s114/control-artifacts/app-run/preflight/result.json` | Binds the previous binary and proves the trace/grader collision occurred before inference. |
| `docs/sprints/s114/control-artifacts/runtime/epoch-4/final/selection.json` | Pins Q4_K_M, 32,768 context, 24 GPU layers, and the selected historical coordinate. |
| `docs/sprints/s114/control-artifacts/runtime/epoch-4/final/runtime-verification.json` | Preserves the prior qualified runtime result that must be repeated for the changed binary and fresh host state. |

`ferric query` currently canonicalizes the workspace through
`ferric_guard::Workspace`, validates configuration, checks, skills, resume
input, and attachments, and only then unconditionally creates
`<workspace>/.ferric/trace`. This order gives Sprint 115 a clean insertion
point: validate an optional external trace directory after the workspace is
known but before any trace-directory mutation. The shared trace-sink helper
must remain unchanged because chat, MCP, API, and other callers also use it.

The reusable path primitives already exist in `ferric-guard`: lexical
normalization, nearest-existing-ancestor canonicalization, and component-wise
containment. They are private and deliberately accept only paths inside a
workspace, so the external trace validator should be query-local rather than
loosening the workspace boundary. A supplied root must be absolutized against
the invocation working directory, resolve its deepest existing ancestor,
reject existing non-directories and every existing symlink/reparse component,
and reject equality or ancestry in either direction against the canonical
workspace. All rejection checks occur before `create_dir_all`; a second
canonical/reparse/disjointness check after creation narrows the remaining
concurrent path-swap race before the trace file is allocated.

Resume validation already proves that the requested workspace equals the
workspace recorded in the source trace. It does not retain an output trace
root, and the printed resume hint currently omits even a non-default
workspace. Therefore an external-trace continuation must explicitly repeat
`--trace-dir`; omission must fail before mutation rather than silently fall
back to the sealed workspace. The printed instruction must include canonical
`--workspace` and, when used, `--trace-dir` with platform-correct quoting.

The frozen MH-RS01 seed, grader, checks profile, and hashes remain reusable.
Sprint 114's selected Qwen3.8-27B UD-Q4_K_M coordinate—context 32,768 and 24
GPU layers—also remains the starting coordinate, while its 3.2065 decoded
tokens/s is historical comparison data rather than a post-reboot result. A
fresh observation after the user's restart found no Ferric/llama process or
port-8080 listener, about 19,566 MiB available system memory, and about 9,184
MiB free on the 11,264 MiB RTX 2080 Ti. Resident applications still include
Steam, Discord, Galaxy, security software, and the active ChatGPT/Codex host;
none will be stopped by this sprint. Runtime fit must therefore be freshly
attested rather than inferred from either the restart or Sprint 114.

Two stale generated trees, `target/s114-experiment/app-harness` and
`target/s114-experiment/self-test-workspaces`, remain. The frozen procedure
requires its four canonical run paths to be absent before a new trial. Any
raw bytes not already retained must be inventoried and preserved before a
guarded exact-path cleanup; broad or unresolved recursive deletion is not
acceptable.

## 3. External Sources

- [Rust `std::fs::canonicalize`](https://doc.rust-lang.org/std/fs/fn.canonicalize.html)
   resolves symbolic links and `..`, but requires the target to exist and
   yields extended-length syntax on Windows. The validator therefore needs a
   nearest-existing-ancestor algorithm and must keep display/CLI paths
   separate from canonical comparison paths.
- [Rust `std::fs::symlink_metadata`](https://doc.rust-lang.org/std/fs/fn.symlink_metadata.html)
   inspects a path without following a symbolic link. It is useful for an
   existing-component walk, but does not by itself identify every Windows
   junction.
- [Microsoft Reparse Point Operations](https://learn.microsoft.com/en-us/windows/win32/fileio/reparse-point-operations)
   specifies `FILE_ATTRIBUTE_REPARSE_POINT` as the test for a file or
   directory with a reparse point. Windows validation must inspect that bit,
   not only Rust's `is_symlink()` result.
- [Rust `std::path` module](https://doc.rust-lang.org/stable/std/path/)
   notes that non-filesystem path comparisons such as `starts_with` are case
   sensitive even on case-insensitive filesystems. Bidirectional overlap
   checks therefore need platform-aware component comparison after
   canonicalization instead of a raw `Path::starts_with` assumption.

No new model-repository survey is needed: Sprint 114 already pinned and
locally acquired the exact Qwen3.8 artifact. This sprint's open questions are
local product semantics and fresh host behavior, not model availability.

## 4. Risks, Unknowns, Dependencies

- **Pre-mutation safety:** equal, descendant, ancestor, `..` alias,
  case-alias, existing-file, symlink, and Windows-junction roots must all fail
  without creating either the requested directory or workspace `.ferric`
  state.
- **Concurrent path replacement:** standard path APIs cannot fully eliminate
  a malicious local process swapping an ancestor between inspection and
  creation. The operator-authored path is not model-controlled; pre-creation
  validation plus post-creation revalidation is the bounded minimum. A fully
  handle-relative external filesystem capability would be a separate design.
- **Resume regression:** accepting an external fresh run while permitting a
  resume to default back into `<workspace>/.ferric/trace` would recreate the
  original blocker. Explicit repetition and mutation-free refusal are gates.
- **Binary comparability:** changing `query.rs` invalidates the Sprint 114
  release-binary identity. Format, clippy, targeted/full tests, backend-enabled
  release build, CLI surface capture, and a fresh runtime smoke must precede
  application inference.
- **Fresh resource state:** improved idle memory does not prove the 27B
  coordinate remains viable under load. Capture cold RAM/VRAM/process/listener
  state, managed-server identity/properties, nonce smoke, bounded throughput,
  and teardown again.
- **Frozen application boundary:** the grader, seed, prompt, checks, and
  allowed output paths cannot change. Once Ferric starts the candidate, Codex
  may inspect evidence but may not edit candidate bytes.
- **Sandbox readiness:** WSL is installed but was stopped after the reboot.
  The WSL/Bubblewrap path must be re-probed before model-authored code is
  executed; an unavailable sandbox is infrastructure failure, not permission
  to run the candidate unconfined.
- **Generated-state cleanup:** stale ignored experiment trees require an exact
  inventory, preservation decision, target-resolution checks, and narrow
  cleanup before the canonical run.
- **Operator-surface scope:** the complete INT-0008 workflow needs durable
  cross-platform state, locking, idempotent status/resume, explain, evidence,
  and ownership-aware cleanup. Adding a platform-specific alias now would
  create another surface to retire and would delay the causal app trial.

## 5. Recommended Approach

1. Implement T-11414 entirely at the query boundary: optional
   `--trace-dir`, unchanged default behavior, platform-aware disjoint-path and
   reparse validation before mutation, post-creation revalidation, explicit
   external-root repetition on resume, and truthful generated resume commands.
2. Add unit and CLI coverage for the default, external fresh run, linked
   external resume, absent-repeat refusal, nonexistent tails, all overlap
   aliases, existing files, symlinks, Windows junctions, quoting, and
   mutation-free rejection. Document query-only semantics and examples.
3. Run Rust formatting, clippy, targeted and full tests; build the
   backend-enabled release binary; bind its hash and help surface; and perform
   fresh managed-server qualification against the already acquired Q4 model.
4. Inventory and preserve any unique stale experiment bytes, then clean only
   the verified generated roots required by the frozen harness. Re-run its
   self-tests and confirm seed/grader/check hashes remain unchanged.
5. Execute T-11410: prepare the candidate from the sealed seed, start the one
   owned server coordinate, run the one-turn first segment with an external
   trace root, require the intended linked continuation, and allow only Ferric
   and the model to mutate the candidate. Run the frozen in-session check,
   final grader, trace verification, tree/effect/journal reconciliation, and
   outcome classification.
6. Execute T-11412 as the Book defines it: archive and hash the completed
   experiment, verify cold teardown, and publish a non-inflated INT-0007
   verdict. Do not repurpose T-11412 as the broader command-surface feature.
7. Preserve the larger INT-0008 compact workflow as an explicitly ordered
   follow-up after the trial. Its target remains one installed cross-platform
   namespace for explain/run/status/resume/evidence/cleanup, not a wrapper
   around Sprint 114's scripts.

## Artifacts

- [Sprint 114 failure report](../../s114/failure-report.md)
- [Frozen app-harness contract](../../s114/control-artifacts/app-harness/README.md)
- [Blocked preflight result](../../s114/control-artifacts/app-run/preflight/result.json)
- [Prior runtime selection](../../s114/control-artifacts/runtime/epoch-4/final/selection.json)
- [Prior runtime verification](../../s114/control-artifacts/runtime/epoch-4/final/runtime-verification.json)
- [T-11414, T-11410, and T-11412 backlog](../../../work/tasks.md#book-v2-carry-forward-from-sprint-114)

The research audit inspected 20 unique project files across the current Book,
query/replay/workspace implementation and tests, frozen app harness, and prior
runtime evidence. It used four primary external sources and stayed within the
phase's file/source budgets; no budget override is required.

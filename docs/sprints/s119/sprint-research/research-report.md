# Sprint 119 Research Report

## Intents Reviewed

- [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) — revised;
  current state: active. AC-6 now explicitly records the owner's source-driven
  execution and source-owned reaping requirement. This sprint advances that
  safety slice; it does not accept the compact model workflow or platform parity.

## 1. Sprint Goal

Review and refactor the uncommitted process-cleanup carryover after Sprint 118,
then prove it through Cargo-driven tests. Consolidate process ownership rather
than preserve two diverging implementations, retain all existing lifecycle
assertions, and remove direct compiled-artifact execution from CI. Close this
bounded reliability increment with one owner-merged `dev` to `main` PR.

## 2. Existing Code Survey

| File | Relevance | Notes |
|------|-----------|-------|
| `Cargo.toml` | high | Shared dependency boundary; command-group rationale is no longer valid. |
| `crates/ferric-bench/Cargo.toml` | high | Process dependency scope. |
| `crates/ferric-bench/src/process.rs` | high | Guarded Windows suspended spawn and file capture; Linux registry/watcher races. |
| `crates/ferric-bench/src/runner.rs` | high | Benchmark child and noisy-output regressions. |
| `crates/ferric-bench/src/verify.rs` | high | Python verification uses bounded process capture. |
| `crates/ferric-bench/src/autonomy.rs` | medium | Interpreter preflight uses same process boundary. |
| `crates/ferric-cli/Cargo.toml` | high | Shared test dependencies. |
| `crates/ferric-cli/src/test_process_containment.rs` | high | Duplicate, weaker Windows spawn/wait and stale Linux PGID snapshot. |
| `crates/ferric-cli/src/test_process_containment_tests.rs` | high | Source-defined tree and exact-parent regressions; pidfd event validation too loose. |
| `crates/ferric-cli/src/server_process.rs` | high | Native retained-process smoke with atomic readiness. |
| `crates/ferric-cli/src/server_registration.rs` | high | Remaining pipe-before-exit and unbounded collection helper. |
| `crates/ferric-cli/src/server.rs` | high | Source lifecycle helper ownership. |
| `crates/ferric-cli/src/bin/ferric_lifecycle_fixture.rs` | high | Exact owner pidfd, bounded fixture lifetime. |
| `crates/ferric-cli/tests/server_lifecycle_fixture.rs` | high | Owner-death test proves exit but not reaping; intentional detached server lifecycle. |
| `crates/ferric-cli/tests/cli.rs` | high | Batch stdin and file capture conversions. |
| `crates/ferric-cli/tests/bench_mock.rs` | high | L0/L1 and L3/L4 model-failure evidence must remain. |
| `crates/ferric-cli/src/cron.rs` | medium | Source-child parent-death setup in carryover. |
| `crates/ferric-cli/src/query.rs` | medium | Carryover import/config changes. |
| `crates/ferric-cli/src/main.rs` | medium | Test module registration. |
| `.github/workflows/ci.yml` | high | Linux lifecycle job directly invokes extracted test executable. |

Two independent read-only reviews identified: command-group's Windows
post-spawn failure window can abandon a suspended child; its wait path accepts
an arbitrary completion-port message rather than proving zero active Job
processes; both Linux registries copy PGIDs outside their locks; pidfd polling
must distinguish POLLIN/POLLHUP from invalid descriptors. The previous claim
that Cargo produces spurious pidfd POLLIN has no demonstrated causal evidence
and must be removed, not repeated as a platform fact.

## 3. External Sources

- [Microsoft Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)
  — nested Job containment, kill-on-close, and queried accounting are stronger
  than assuming a completion notification means all processes are gone.
- [Rust CommandExt](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html#method.process_group)
  — standard-library process groups remove the Unix need for command-group.
- [Linux pidfd_open](https://man7.org/linux/man-pages/man2/pidfd_open.2.html)
  — exact descriptors are pollable; POLLIN includes zombies, POLLHUP proves reaping.
- [Linux subreapers](https://man7.org/linux/man-pages/man2/PR_SET_CHILD_SUBREAPER.2const.html)
  — adopted descendants require an actual wait/reaper, not merely kill/exit.

## 4. Risks, Unknowns, Dependencies

- **Risk:** Killing a Linux process that owns a nested, separate group with
  immediate PDEATHSIG SIGKILL prevents its watcher from cleaning that group.
  POSIX groups are not Windows Jobs and do not contain deliberate group escape.
  The shared API must describe this boundary truthfully. Controlled Linux
  cancellation fixtures need an outer source supervisor/reaper or namespace;
  arbitrary hostile escape/owner-SIGKILL containment is deferred, not accepted.
- **Risk:** A process-wide subreaper must never reap unrelated direct children;
  any adopted-child waits must be scoped to the owned group and serialized.
- **Risk:** Managed lifecycle tests intentionally detach a server from its
  launcher. Replacing their lifetime-spanning fixture owner with an immediately
  killed launcher group would invalidate the positive lifecycle behavior.
- **Risk:** New shared code must preserve timeout/error distinctions, output
  bounds, exact ownership, failed-spawn rollback, and all original assertions.
- **Dependency:** Native Windows execution is available locally; Linux behavior
  needs Cargo-driven Linux/CI verification. macOS acceptance is not implied.
- **Dependency:** A pre-existing change in Sprint 114's model acquisition JSON
  belongs to the user and is excluded from this sprint. Its SHA-256 at intake
  is `8ecf94878e7ad745aea28a9365af58ee111c80b26d21a15a0f434edb2beb75db`.
  The Book clean-state gate cannot silently absorb or discard that change.
- **Deferred:** T-11806 and T-11808 through T-11812, the real-model app trial,
  live Tailscale acceptance, new models, and unrelated dependency-updater PRs.

## 5. Recommended Approach

Primary: one small `ferric-process` crate for bounded capture and owned native
process scopes. Reuse the guarded Windows design, remove command-group, and
make CLI test adapters thin. Correct Linux registry and exact-parent ownership,
prove reaping in controlled fixtures, preserve detached lifecycle semantics,
and run the namespace CI command through Cargo. Add focused failure/cancellation
regressions before the workspace gates and final adversarial phase audit.

Alternative considered: repair both implementations independently. Rejected
because divergence already produced materially different cleanup guarantees.
Do not redesign all production subprocesses or claim arbitrary Linux process
escape containment as part of this bounded refactor.

## Artifacts

- [Sprint 118 reconciliation](../../s118/post-merge-carryover.md) — verified
  merged/dirty boundary, without rewriting prior byte-bound evidence.
- [Independent review findings](review-findings.md) — read-only reviewers'
  concrete findings and consolidation recommendation.

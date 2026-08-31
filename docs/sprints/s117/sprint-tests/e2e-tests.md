# Sprint 117 End-to-End Test Evidence

**Intent:** [INT-0008 — Unified local-model workflow](../../../intents/INT-0008-unified-local-model-workflow.md),
AC-3/4/6/7 and enabling evidence toward AC-9.

**Plans:** [finalized build plan](../sprint-plans/build-plan.md) and
[finalized test plan](../sprint-plans/test-plan.md).

## Evidence identity

- **Immutable implementation head:**
  `44f36a239f48c4c230e0454535451ecf62e5ffa3`.
- **Authoritative CI run:**
  [33346491895](https://github.com/crussella0129/Animus_Ferric/actions/runs/33346491895),
  push to `dev`, success, 2026-08-31T01:05:11Z through 01:09:04Z.
- **Local runner:** Windows x64 with Rust 1.96.0.

## Frozen end-to-end rows

| Clause | Exact command | Local result |
|---|---|---|
| E05-C | `cargo test -p ferric-cli --all-features model_free_server_lifecycle_fixture_e2e -- --nocapture` | Exit 0; exactly 1 passed in 2.84s. Real CLI `up/status/down` and stale-local/live-global recovery used the closed-name Rust fixture and asserted no owned process, listener, registration, stage, coordination residue, or sentinel mutation. |
| E05-D | `cargo test -p ferric-cli --all-features tailscale_mode_refuses_before_side_effects -- --nocapture` | Exit 0; exactly 1 passed in 2.85s. Real `up/doctor/status/down` cases left engine/Tailscale invocation markers absent and preserved process, listener, records, and sentinels. |

These are rows 19 and 18 respectively in the locked nineteen-row acceptance
API. The other seventeen rows are in [unit evidence](unit-tests.md#frozen-clause-matrices).

## Supplemental CLI lifecycle — E04-E/E04-A

`cargo test -p ferric-cli --all-features --locked
legacy_adoption_then_down_cli_e2e -- --nocapture` exited 0 with exactly one
pass in 0.88s. A live v1 fixture first made status/down render complete
adoption guidance without signalling; real adopt then published current
identity and real down stopped that retained generation and removed unchanged
aliases.

The explicit serialized local gate

```text
cargo test -p ferric-cli --features lifecycle-fixture --test server_lifecycle_fixture --locked -- --test-threads=1
```

exited 0 with all 3 tests passing in 6.64s. The harness used its process-wide lock,
pre-call lifetime tokens, 30-second CLI watchdog, independent HTTP handling,
and diagnosed-only bind retry.

## Immutable CI matrix

| Job | Runner and result | Evidence |
|---|---|---|
| [99351509330](https://github.com/crussella0129/Animus_Ferric/actions/runs/33346491895/job/99351509330) | `lifecycle fixture (windows-latest)`; success in 2m17s | Strict fixture Clippy passed; serialized native Windows fixture passed 3/3 in 5.35s. |
| [99351509375](https://github.com/crussella0129/Animus_Ferric/actions/runs/33346491895/job/99351509375) | `lifecycle fixture (ubuntu-latest)`; Ubuntu 24.04.4 LTS image `20260823.283.1`; success in 42s | Strict fixture Clippy passed. CI built the test as the runner, created PID/network/proc namespaces with `sudo -n unshare`, enabled loopback, then used `setpriv` to restore the runner UID/GID, clear groups, set `no_new_privs`, and empty inheritable/ambient/bounding capability sets. The serialized payload passed the same 3/3 in 1.56s. |
| [99351509362](https://github.com/crussella0129/Animus_Ferric/actions/runs/33346491895/job/99351509362) | `aarch64-unknown-linux-gnu check`; success in 34s | Workspace and lifecycle-feature/all-target surfaces compiled. This is not runtime evidence. |
| [99351509268](https://github.com/crussella0129/Animus_Ferric/actions/runs/33346491895/job/99351509268) | Windows fmt + Clippy + workspace test; success in 3m48s | Ordinary repository regression gate, including the corrected default-parallel HANDLE-release proof. |
| [99351509302](https://github.com/crussella0129/Animus_Ferric/actions/runs/33346491895/job/99351509302) | Ubuntu fmt + Clippy + workspace test; success in 1m53s | Ordinary repository gate explicitly logged the Linux pidfd, non-UTF-8 argv, and unreadable-peer fail-closed regressions as passing. |
| [99351509210](https://github.com/crussella0129/Animus_Ferric/actions/runs/33346491895/job/99351509210) | Ubuntu `backend-openai` Clippy; success in 31s | Feature-gated backend remained warning-free. |

## Failed-attempt correction record

The predecessor run
[33341997210](https://github.com/crussella0129/Animus_Ferric/actions/runs/33341997210)
at head `6eab44bc6a330b756d10c1a48f5bf9c0eff3d115` is retained as a failed attempt,
not acceptance evidence. Its Ubuntu lifecycle job
[99339198027](https://github.com/crussella0129/Animus_Ferric/actions/runs/33341997210/job/99339198027)
passed 1/3 and failed the legacy-adoption and model-free cases after ordinary
host-namespace enumeration encountered:

```text
listener owner enumeration is incomplete because PID 1 is uninspectable:
read /proc/1/fd: Permission denied (os error 13)
```

Ferric correctly failed closed. The correction did not skip unreadable peers
or weaken production authority. It preserved managed-`up` stdout/stderr for
diagnosis and moved only the positive Ubuntu fixture into a namespace where
all relevant peers are visible.

The next green run,
[33343005856](https://github.com/crussella0129/Animus_Ferric/actions/runs/33343005856)
at `c574e8a5216cb687b44a05eeff65e251b401c6af`, proved that namespace
correction but is still superseded acceptance evidence. Adversarial review
then found incomplete in-body EARS matrices and a Windows stale-HANDLE test
oracle that could flake under parallel numeric-handle reuse. Those gaps were
closed without weakening production authority. The deterministic correction,
eight 255-test parallel requalifications, and final CI run are recorded in
[unit evidence](unit-tests.md#failed-attempt-correction-records). Final run
`33346491895` passed all six jobs at `44f36a2`.

## Linux evidence boundary

The successful Ubuntu job proves a non-root, capability-free test payload in
PID/network/proc namespaces prepared with passwordless `sudo`. It does **not**
prove that an ordinary user can create those namespaces or that a process in a
shared/default host PID namespace can inspect every unrelated
`/proc/<pid>/fd`. Positive lifecycle behavior is proved where all relevant
namespace peers are visible. Incomplete peer visibility remains deliberately
non-authorizing and is separately proved by
`linux_uninspectable_shared_listener_owner_is_not_exclusive` in the ordinary
Ubuntu workspace job at the immutable head.

This is a real operability boundary to carry forward for an architectural
authority design; it is not evidence of a safety bypass, macOS parity, or
AArch64 runtime support.

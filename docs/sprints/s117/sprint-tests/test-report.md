# Sprint 117 Test Report

## Verdict

**Pass for Sprint 117's lifecycle acceptance-recovery scope.** All nineteen
frozen EARS commands passed at immutable implementation head
`44f36a239f48c4c230e0454535451ecf62e5ffa3`; the independent final
[test critique](critique.md) is `clean`; and authoritative CI run
[33346491895](https://github.com/crussella0129/Animus_Ferric/actions/runs/33346491895)
passed all six jobs.

This closes the clause-level evidence gap left by Sprint 116 for the affected
server-lifecycle portions of INT-0008 AC-3, AC-4, AC-6, and AC-7. It provides
enabling evidence toward AC-9 but does not complete the wider compact local-
model workflow, cross-platform AC-8, macOS support, or an actual model-backed
application trial. INT-0008 therefore remains `active`, not `realized`.

## Acceptance result

| Scope | Result | Evidence |
|---|---|---|
| T-11701 / E01-A through E01-D | pass | Lossless two-scope/origin inventory, strict schema authority, real multi-process coordination, and atomic conditional removal matrices passed. |
| T-11702 / E02-A through E02-C | pass | Retained Windows HANDLE/Linux pidfd identity, exact listener classification, and child-binding-window matrices passed; destructive authority remains fail-closed. |
| T-11703 / E03-A through E03-C | pass | Typed cross-workspace resolution, exact status rendering, and all lifecycle consumers propagated Empty/Ready/Degraded/StaleOnly/Conflict/Unverifiable without fallback. |
| T-11704 / E04-A through E04-E | pass | Handle-only teardown, exit/listener postconditions, ordered conditional cleanup, non-mutating ambiguity, and explicit legacy adoption passed. |
| T-11705 / E05-A and E05-B | pass | Durable no-clobber publication and exact partial-publication compensation passed every injected persistence/runtime boundary. |
| T-11706 / E05-C and E05-D | pass | Real CLI model-free lifecycle/stale recovery and Tailscale pre-side-effect refusal passed locally and in serialized Windows/isolated non-root Linux CI fixtures. |

Detailed command and assertion provenance is retained in
[unit/static](unit-tests.md), [integration](integration-tests.md), and
[end-to-end](e2e-tests.md) evidence.

## Canonical confirmations

- Both frozen Presence commands passed and exposed all nineteen names.
- Each frozen clause filter passed exactly one intended test.
- `cargo test -p ferric-cli --all-features --locked`: 336 passed, 0 failed.
- `cargo test --workspace --all-features --locked`: 1,089 passed, 0 failed,
  4 intentional ignores.
- The serialized lifecycle fixture passed 3/3 locally; CI repeated 3/3 on
  Windows and in an isolated non-root, capability-free Ubuntu payload.
- Frozen fmt/Clippy gates, strict workspace Clippy, both AArch64 compile
  checks, and `git diff --check` passed.
- The protected Sprint 114 acquisition artifact remained unstaged with
  SHA-256
  `8ECF94878E7AD745AEA28A9365AF58EE111C80B26D21A15A0F434EDB2BEB75DB`.

## Reliability and correction record

The failed Ubuntu lifecycle attempt at `6eab44b` exposed an honest authority
boundary: ordinary host-namespace `/proc` enumeration could not inspect PID 1,
so Ferric correctly refused positive ownership. The accepted fixture prepares
an isolated PID/network/proc namespace, then runs the payload as the ordinary
runner UID/GID with no capabilities. Production authority was not weakened.

Before the final implementation commit, default-parallel Windows testing also
exposed a flaky stale-numeric-HANDLE test oracle. The proof now checks the real
`CloseHandle` result through the same close path used by `Drop`, followed by
eight consecutive 255/255 parallel runs, full local suites, and green Windows
CI. Both failed attempts remain recorded rather than being hidden by the final
pass.

## Evidence boundary and carry-forward

- Positive Linux lifecycle E2E is proved only where every relevant namespace
  peer is visible. Incomplete ordinary-host peer visibility remains
  non-authorizing and has a passing negative regression.
- AArch64 is compile-only evidence; there is no AArch64 runtime result.
- No macOS lifecycle parity is claimed.
- The fixture uses a closed-name Rust server and proves lifecycle semantics,
  not inference quality, GGUF compatibility, calibration, or the deferred
  model-backed medium-horizon application task.
- The broader local-model backlog may resume after Loop closes T-11606; its
  later tasks still own bounded calibration, runtime discovery, reasoning and
  compaction controls, and the small human command surface.

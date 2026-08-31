# Sprint 118 Research Report

## Intents Reviewed

- [INT-0008 — Unified local-model workflow](../../../intents/INT-0008-unified-local-model-workflow.md) — selected. T-11510 is the first item in the authoritative ordered local-model backlog and advances AC-3 (explicit partial state), AC-4 (truthful status), AC-6 (exact ownership), and AC-7 (bounded cleanup), with enabling evidence toward AC-9.

No new intent is required. The desired positive Tailscale lifecycle and its
ownership boundary are already part of active INT-0008. This sprint does not
claim AC-8 platform parity; T-11707 separately owns that remaining boundary.

## 1. Sprint Goal

Complete T-11510: restore `ferric server up --tailscale` only with durable,
endpoint-scoped ownership of the Tailscale Serve mutation. Ferric must capture
the exact relevant pre-state before mutation, publish recovery authority before
the mutation can escape, apply and verify one collision-safe endpoint, report
its state truthfully, and compare-remove only that unchanged endpoint on every
later launch failure and verified `server down`.

The sprint is deliberately bounded. It does not add benchmark timeout/output
controls (T-11505), hardware calibration, model aliases, the compact workflow,
live-tailnet acceptance, or broader platform authority. It must never call or
recommend `tailscale serve reset`, and it must not use service-wide
`get-config`/`set-config` as a substitute for node-level endpoint ownership.

## 2. Existing Code Survey

| Source | Finding and consequence |
|---|---|
| [`docs/work/tasks.md`](../../../work/tasks.md#post-sprint-115--ordered-local-model-work) | T-11510 is first in the explicit ordered backlog; T-11505 and later items remain out of scope. |
| [`docs/intents/INT-0008-unified-local-model-workflow.md`](../../../intents/INT-0008-unified-local-model-workflow.md) | AC-3/4/6/7 define the lifecycle acceptance boundary. AC-8 remains separately gated and must not be claimed. |
| [Sprint 117 research](../../s117/sprint-research/research-report.md) and [accepted test report](../../s117/sprint-tests/test-report.md) | Sprint 117 intentionally retained zero-side-effect Tailscale refusal until exact native process/listener authority was accepted. That prerequisite now has clause-level evidence. |
| [`crates/ferric-cli/src/server.rs`](../../../../crates/ferric-cli/src/server.rs) | `ServerRunfile` schema v2 records only a `tailscale: bool`; `up` and doctor refuse `--tailscale` before all probes, while status/down block every `tailscale: true` record. The launch pipeline already retains the child, verifies health/identity/listener ownership, atomically publishes mirrored registration bytes, and compensates publication failures. |
| [`crates/ferric-cli/src/server_resolution.rs`](../../../../crates/ferric-cli/src/server_resolution.rs) | Resolution treats every `tailscale: true` record as unverifiable. Positive support needs typed ownership state while legacy boolean-only records remain fail-closed. |
| [`crates/ferric-cli/src/server_registration.rs`](../../../../crates/ferric-cli/src/server_registration.rs) | Exact-byte compare/remove and compare/replace helpers already provide the registration-side concurrency boundary. Serve cleanup needs the same compare-before-mutate discipline. |
| [`crates/ferric-cli/tests/server_lifecycle_fixture.rs`](../../../../crates/ferric-cli/tests/server_lifecycle_fixture.rs) | The fixture already provides isolated fake engine/Tailscale executables, but current Tailscale coverage proves refusal only. It is the right seam for exact argv, failure, concurrency, and preservation matrices without a live tailnet. |
| [`docs/server-configuration.md`](../../../server-configuration.md#loopback-only-by-design) | Operator documentation already states the intended contract: capture, own, conditionally restore/remove, preserve ambiguous recovery evidence, and never perform a blind node-wide reset. |

The critical crash window is between a successful Serve mutation and durable
registration publication. Reusing the current order unchanged would allow an
external endpoint to survive with no Ferric recovery authority. The durable
record therefore has to be write-ahead: publish the exact desired endpoint and
ownership token before invoking the mutating Tailscale command. Its phase can
then be derived from observed Serve state rather than advanced through another
fallible mirrored-record transition.

A unique HTTPS mount path is a narrower coordinate than a node-wide port or
root handler. A candidate coordinate is
`/_ferric/<high-entropy-ownership-token>` on HTTPS 443, targeting
`http://127.0.0.1:<managed-port>`. The owner is the exact path handler and
target, not shared HTTPS/TCP scaffolding or unrelated handlers on the node.

## 3. External Sources

1. [Tailscale Serve CLI reference](https://tailscale.com/docs/reference/tailscale-cli/serve) (last validated 2026-01-26) documents `serve status --json`, `--set-path`, background persistence, the loopback-only reverse-proxy target, and endpoint-scoped `off` using the original flags. It also states that `serve reset` clears the current Serve configuration and that service `get-config`/`set-config` operate on Services. Therefore Ferric should use one unique path, inspect node-level status JSON, and disable only that path with the original flags.
2. [Tailscale `serve_v2.go`](https://github.com/tailscale/tailscale/blob/main/cmd/tailscale/cli/serve_v2.go) confirms the current CLI implementation reads the live Serve configuration for ordinary `serve` mutations while the exported set/get configuration surface is service-oriented. This reinforces an adapter around documented `status --json`, exact `serve`, and exact `off`, rather than whole-node replacement.
3. [Tailscale Serve request routing](https://github.com/tailscale/tailscale/blob/main/ipn/ipnlocal/serve.go) selects the longest matching handler and strips a non-root mount prefix before reverse-proxying. Therefore a tokenized external base such as `https://<node>/_ferric/<token>/v1` preserves the backend's `/v1` request path while making Ferric's owned handler externally unique.
4. [Tailscale `whoami` implementation](https://github.com/tailscale/tailscale/blob/main/cmd/tailscale/cli/whoami.go) returns the current self node in JSON and was introduced in Tailscale 1.102.1. Ferric can parse only the canonical `Node.Name` FQDN, avoid peer inventory, and fail closed rather than inventing a hostname.
5. [`getrandom::fill` 0.4 API](https://docs.rs/getrandom/latest/getrandom/fn.fill.html) fills a caller-owned buffer from the operating system's preferred random source and reports partial reads as failure. Ferric can therefore request exactly 128 bits for the external coordinate and fail before all engine or Tailscale side effects if entropy is unavailable.

No external artifact was copied into the repository. These five primary sources
are design evidence, not acceptance evidence; the frozen tests must prove
Ferric's behavior with a deterministic fake CLI.

## 4. Risks, Unknowns, and Dependencies

- **Write-ahead publication:** if the ownership record is not durable before
  mutation, a crash can orphan external state. If publication only partially
  succeeds, no Serve mutation is authorized and ordinary publication
  compensation must complete first.
- **Concurrent replacement:** a path that no longer has the exact recorded
  target is not Ferric-owned. External-path cleanup must stop and report the
  mismatch; Ferric may still stop/reap the independently authorized exact
  child, but it must retain registrations until the external path is resolved.
- **Status schema drift:** `serve status --json` is documented, but its nested
  node-level shape can evolve. Parsing must be strict about the claimed
  coordinate and tolerant of unrelated fields; malformed, ambiguous, or
  unreadable output fails closed.
- **Shared scaffolding:** HTTPS 443 and its node certificate/configuration can
  be shared. Ferric owns only its unique handler path. Full JSON equality would
  wrongly convert unrelated changes into ownership, while a reset would destroy
  them.
- **Failure ordering:** apply failure, post-apply verification failure,
  post-publication child/listener failure, `off` failure, and post-`off`
  verification failure each require different evidence retention. No error is
  allowed to imply that cleanup succeeded.
- **Independent cleanup authority:** proxy ambiguity does not invalidate the
  already-proven exact child identity. Failure compensation and down should
  still stop/reap that exact child to end exposure, but must retain the
  ownership journal until the external path is proved absent.
- **CLI compare/delete boundary:** endpoint-scoped `off` performs its own
  optimistic config update but cannot atomically bind that update to Ferric's
  immediately preceding target comparison. The high-entropy token path makes
  accidental collision infeasible and is sufficient for ordinary concurrent
  operation; hostile replacement of the exact token coordinate inside that
  narrow window would require a future direct LocalAPI `If-Match` adapter.
- **Snapshot scope:** the canonical whole-document status digest is retained as
  provenance, not mutation authority. Launch and cleanup decisions compare the
  exact token path, target, and compatible web-port mode; unrelated-handler
  changes neither authorize nor block Ferric's coordinate-scoped mutation.
- **Legacy records:** historical `tailscale: true` records lack an endpoint
  token. They must retain the current manual, non-mutating recovery path rather
  than being upgraded by inference.
- **Idempotency:** a repeated cleanup must accept an already-absent exact path,
  but a repeated launch must not silently adopt an existing path or overwrite a
  concurrent handler.
- **Platform and environment:** the adapter must invoke the current executable
  consistently on Windows and Unix, but this sprint's model-free fixtures do
  not prove a real tailnet, macOS parity, ACLs, MagicDNS, or certificate
  issuance. Tailscale older than 1.102.1 also lacks the selected self-identity
  command and must fail with upgrade guidance. Those remain explicit
  limitations.

The accepted Sprint 117 native process/listener authority is a dependency:
Ferric attempts proxy comparison/removal first, then independently applies the
unchanged retained-handle teardown boundary to the exact owned process. It may
remove the ownership journal only after both resources are resolved.

## 5. Recommended Approach

1. Add a small Tailscale Serve adapter with typed status projection, command
   execution, deterministic test injection, bounded self-FQDN discovery, and
   exact apply/off argv. Extract
   only the unique mount handler; never expose reset or whole-config mutation.
2. Extend schema v2 additively with optional Serve ownership containing a
   version, high-entropy token, HTTPS port, mount path, loopback target,
   canonical self FQDN, remote base, pre-status digest, and the exact verified
   handler projection. Keep the ordinary runfile `base_url` loopback-local and
   keep `tailscale: bool` for compatibility; boolean-only true records remain
   blocked.
3. For Tailscale launch, complete ordinary static/process readiness checks,
   prove the unique path absent, publish the mirrored ownership record as a
   write-ahead recovery token, apply the exact path, and verify that the path
   equals the recorded target. Then repeat the existing retained-child and
   listener authority check before reporting success.
4. On every failure after write-ahead publication, inspect the exact path. If
   it is absent, proceed with ordinary exact-child compensation. If it still
   equals the recorded target, run endpoint-scoped `off` and verify absence
   before child compensation. If it differs or cannot be inspected, do not
   mutate that path; independently stop/reap only the already-proven exact
   child and retain registrations as recovery evidence.
5. Teach resolution/status/down about typed ownership. Status reports
   pending/active/absent/mismatch/uninspectable proxy state truthfully. Down
   compares or cleans the proxy first, then independently reuses the accepted
   exact native process teardown. It compare-removes registrations only when
   both resources are resolved; otherwise it retains them for retry. Legacy
   state remains wholly fail-closed.
6. Freeze model-free unit and fake-CLI integration matrices for unrelated
   pre-state preservation, path collision, write-ahead ordering, apply and
   verification failures, concurrent replacement, exact off, retry/idempotency,
   malformed output, exact operator messages, and the invariant that no reset
   or blind set-config command can occur. Update operator docs and backlog
   state only after those matrices pass.

### Referenced artifacts

- The sprint uses the five linked external primary sources directly; it saved no
  additional research artifact under `docs/sprints/s118/`.
- The protected Sprint 114 acquisition evidence is unrelated and must remain
  byte-identical and unstaged throughout this sprint.

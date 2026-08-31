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
| [`crates/ferric-cli/tests/server_lifecycle_fixture.rs`](../../../../crates/ferric-cli/tests/server_lifecycle_fixture.rs) | The fixture already provides an isolated fake engine, while current Tailscale coverage proves refusal only. A loopback fake LocalAPI is the correct test seam for exact HTTP ordering, ETag/CAS, failure classification, concurrency, and preservation matrices without a live tailnet. |
| [`docs/server-configuration.md`](../../../server-configuration.md#loopback-only-by-design) | Operator documentation already states the intended contract: capture, own, conditionally restore/remove, preserve ambiguous recovery evidence, and never perform a blind node-wide reset. |

The critical crash window is between a successful Serve mutation and durable
registration publication. Reusing the current order unchanged would allow an
external endpoint to survive with no Ferric recovery authority. The durable
record therefore has to be write-ahead: publish the exact desired endpoint and
ownership token before sending the LocalAPI mutation, initially with
`apply_confirmed=false`. Only an exact post-apply observation can authorize a
compare-and-replace transition of every unchanged journal to
`apply_confirmed=true`; that durable phase transition must finish before later
scoped cleanup can rely on ordinary absent-state convergence. A post-send
LocalAPI failure plus an immediate absent observation is not a completion
barrier, so the unconfirmed journal remains recovery authority rather than
being deleted.

A unique HTTPS mount path is a narrower coordinate than a node-wide port or
root handler. A candidate coordinate is
`/_ferric/<high-entropy-ownership-token>` on HTTPS 443, targeting
`http://127.0.0.1:<managed-port>`. The owner is the exact path handler and
target, not shared HTTPS/TCP scaffolding or unrelated handlers on the node.

## 3. External Sources

- [Tailscale Serve CLI reference](https://tailscale.com/docs/reference/tailscale-cli/serve) (last validated 2026-01-26) documents background Serve persistence, loopback-only reverse-proxy targets, path-scoped removal, and the destructive node-wide scope of `serve reset`. It establishes the operator semantics and the prohibition on blind reset; it is not Ferric's mutation interface.
- [Tailscale v1.102.2 `serve_v2.go`](https://github.com/tailscale/tailscale/blob/v1.102.2/cmd/tailscale/cli/serve_v2.go) confirms that ordinary endpoint operations are implemented as a read/modify/write of the live Serve configuration with an ETag-bearing POST and may also normalize scaffolding or Funnel state. The corrected design therefore performs its own narrowly checked LocalAPI CAS instead of relying on a separate CLI process boundary.
- [Tailscale v1.102.2 Serve configuration semantics](https://github.com/tailscale/tailscale/blob/v1.102.2/ipn/serve.go) show trailing-slash handler normalization, expected-host Funnel-key deletion, last-handler TCP cleanup, and foreground-over-background resolution. Ferric must reject every pre-state where an endpoint mutation could normalize, shadow, replace, or delete state outside its exact token/target authority.
- [Tailscale v1.102.2 LocalAPI Serve handler](https://github.com/tailscale/tailscale/blob/v1.102.2/ipn/localapi/serve.go) returns the raw Serve configuration with its SHA-256 ETag and accepts whole-config POST only under `If-Match`. GET and POST are independent requests, so a transport failure after POST begins followed by an absent GET does not prove the accepted request can never commit; HTTP 412, by contrast, proves no mutation.
- [Tailscale v1.102.2 Serve backend](https://github.com/tailscale/tailscale/blob/v1.102.2/ipn/ipnlocal/serve.go) serializes configuration access under the backend lock, routes by longest matching handler, and strips a non-root mount prefix before reverse-proxying. Inference: a later GET can acquire the lock before a delayed POST reaches it, while a tokenized external base still preserves the backend `/v1` path once the exact handler is active.

No external artifact was copied into the repository. These five primary sources
are the bounded Research-phase set; the post-Loop adversarial review records
supplemental pinned identity, FQDN, LocalAPI, and transport sources. Sources are
design evidence, not acceptance evidence; the frozen tests must prove Ferric's
behavior with a deterministic fake LocalAPI.

## 4. Risks, Unknowns, and Dependencies

- **Write-ahead publication:** if the ownership record is not durable before
  mutation, a crash can orphan external state. If publication only partially
  succeeds, no Serve mutation is authorized and ordinary publication
  compensation must complete first.
- **Concurrent replacement:** a path that no longer has the exact recorded
  target is not Ferric-owned. External-path cleanup must stop and report the
  mismatch; Ferric may still stop/reap the independently authorized exact
  child, but it must retain registrations until the external path is resolved.
- **Schema and identity drift:** normal operations must pin capability 142 and
  Tailscale version core 1.102.2, reject duplicate/unknown/wrongly typed
  authority-relevant JSON (including null handler objects), and bind each
  configuration to a same-session status/config/status identity sandwich.
  Malformed, ambiguous, shadowed, or unreadable state fails closed.
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
- **CAS and failure classification:** the configuration response ETag must equal
  the SHA-256 of its exact raw body. Ferric may send one `If-Match` POST and may
  not retry it. HTTP 412 is a definite no-mutation result; an I/O, protocol, or
  daemon failure after any POST bytes are sent is indeterminate and retains the
  journal.
- **Snapshot scope:** the whole-document ETag is mutation authority for one
  checked revision; the journaled digest remains provenance. Apply and cleanup
  compare the exact token path, target, identity, hazards, and scaffold
  provenance while the CAS preserves concurrent unrelated changes by refusing
  the stale revision.
- **Identity/CAS atomicity limit:** the Serve-config ETag is not bound to the
  status StableNodeID or FQDN. Same-connection status sandwiches detect a
  profile switch before or after mutation, but cannot make identity and the
  POST atomic. A switch after the pre-POST status check can therefore mutate a
  different profile if it independently presents the same ETag; the post-check
  must fail closed, attempt scoped compensation, and retain journals. Operators
  must not switch Tailscale profiles during Ferric up/down.
- **Legacy records:** historical `tailscale: true` records lack an endpoint
  token. They must retain the current manual, non-mutating recovery path rather
  than being upgraded by inference.
- **Idempotency:** a repeated cleanup may accept an already-absent path only
  after the durable journal proves an earlier exact apply. An unconfirmed apply
  followed only by absence remains held until the delayed exact path is
  observed and removed or separate daemon-generation/manual proof establishes
  that the request cannot still land. A repeated launch must not silently adopt
  an existing path or overwrite a concurrent handler.
- **Version-drift cleanup:** normal operations remain exactly pinned. Cleanup
  on a later major-1 daemon may make one best-effort CAS that removes only the
  exact handler, preserves all scaffolding and unknown JSON, and refuses if
  reserialization could alter an unknown numeric value. It may not infer that
  a future schema has no other effective route, so even an observed absent
  handler remains unresolved and keeps the journals held.
- **Platform and environment:** conventional Linux installations use
  `/var/run/tailscale/tailscaled.sock` and Windows uses the protected named
  pipe; the invoking account still needs permission to open that endpoint.
  Relocated Linux socket layouts and macOS's token-and-port LocalAPI discovery
  are not implemented and are explicitly unsupported. Model-free fixtures do
  not prove a real tailnet, ACLs, MagicDNS, certificate issuance, or live
  Windows pipe behavior.

The accepted Sprint 117 native process/listener authority is a dependency:
Ferric attempts proxy comparison/removal first, then independently applies the
unchanged retained-handle teardown boundary to the exact owned process. It may
remove the ownership journal only after both resources are resolved.

## 5. Recommended Approach

1. Add a bounded platform-native LocalAPI client: Linux Unix-domain socket,
   Windows protected named pipe, and explicit macOS refusal. Pin normal
   operations to capability 142 and Tailscale version core 1.102.2. Expose no
   generic command runner or arbitrary production endpoint override.
2. Extend schema v2 additively with optional Serve ownership containing a
   version, high-entropy token, HTTPS port, mount path, loopback target,
   canonical self FQDN, remote base, pre-status digest, and the exact verified
   handler projection plus an explicit durable apply-confirmation phase. Keep
   the ordinary runfile `base_url` loopback-local and keep `tailscale: bool` for
   compatibility; boolean-only true records remain blocked.
3. For Tailscale launch, use one same-session
   `status -> serve-config -> status` sandwich to bind stable node ID, FQDN,
   HTTPS authority, and the exact raw-body ETag. After strict schema and hazard
   validation and unconfirmed write-ahead publication, send one `If-Match` CAS,
   repeat the identity/configuration sandwich, verify the exact path/target,
   and conditionally promote every unchanged mirror to confirmed. Then repeat
   the retained-child and listener authority check before reporting success.
4. Classify pre-send and HTTP-412 failures as definite no-mutation; classify
   post-send failures without a no-op response as indeterminate and never retry.
   Cleanup requires the same stable node ID and exact journaled path/target,
   performs one fresh `If-Match` CAS, and preserves unrelated JSON. On a later
   compatible major-1 daemon, remove the handler only, retain all scaffolding,
   and refuse numeric-loss risk. Retain evidence for mismatch, ambiguity,
   indeterminate absence, or any residual effective route shadow.
5. Teach resolution/status/down about typed ownership. Status reports
   pending/active/absent/mismatch/uninspectable proxy state truthfully. Down
   compares or cleans the proxy first, then independently reuses the accepted
   exact native process teardown. It compare-removes registrations only when
   both resources are resolved; otherwise it retains them for retry. Legacy
   state remains wholly fail-closed.
6. Freeze model-free unit and fake-LocalAPI integration matrices for exact HTTP
   order, same-session identity races, ETag mismatch/412/no-retry behavior,
   post-send ambiguity, unrelated JSON preservation, null/duplicate/unknown
   schema states, true Funnel, effective foreground, descendant/alias hazards,
   version-drift handler-only cleanup, residual shadows, write-ahead ordering,
   and operator messages. Update docs and backlog only after those matrices
   pass.

## Post-Loop supersession

The locked Sprint 118 plan records CLI-specific argv and process-runner clauses.
The post-Loop adversarial pass found that boundary insufficient for hostile
compare-and-set ownership and ambiguous completion. Those clauses remain
immutable provenance, but the correction supersedes them with the direct
LocalAPI design above. The separate post-Loop adversarial review records the
deviation, correction, and verification evidence.

## Artifacts

- The Research phase uses the five linked external primary sources directly.
  Supplemental pinned sources discovered after Loop are preserved in the
  [post-Loop adversarial review](../post-loop-adversarial-review.md).
- The protected Sprint 114 acquisition evidence is unrelated and must remain
  byte-identical and unstaged throughout this sprint.

# Test Critique — Sprint 118

## Review result

The mandatory post-evidence adversarial pass examined every Sprint 118 phase,
and parallel independent code/security reviews examined final tested code head
`d5e61b7f951ca838ea2aed7cefaa2468282bb164`. That pass initially found P2
implementation and evidence defects, reopened Loop, and required corrections.
The final re-review found no remaining P0, P1, or P2 issue. The following P3
limitations are retained rather than overstated as closed.

## Concerns

### C-001 [P3]: Positive production-native transport E2E is incomplete

- **Where:** `tailscale_localapi.rs` Linux UDS/Windows pipe transports and the
  lifecycle fixture
- **Failure mode:** platform evidence
- **Evidence:** the positive five-process lifecycle uses the test-only loopback
  TCP seam. Windows executes a real named-pipe negative
  timeout/cancellation/poisoning path; Linux compiles the UDS path, but neither
  platform executes a successful native transport exchange.
- **Why it matters:** framing, timeout, and cancellation mechanics are tested,
  but successful request/response interoperability through a Linux UDS or
  Windows protected pipe is not.
- **Disposition:** T-11810 owns positive native fake/real-daemon proof; do not
  claim Linux-UDS, Windows-pipe, or AC-8 acceptance from the TCP seam.

### C-002 [P3]: Unix socket `CLOEXEC` is applied in two steps

- **Where:** Unix LocalAPI connection setup
- **Failure mode:** narrow descriptor-inheritance race
- **Evidence:** the Unix socket file descriptor is marked close-on-exec after
  socket creation rather than by an atomic socket flag.
- **Why it matters:** a concurrent fork/exec in the small interval could inherit
  the descriptor. Ferric's current launch flow is not known to fork there, but
  the primitive is not race-free for arbitrary future multithreaded callers.
- **Disposition:** follow up with an atomic `SOCK_CLOEXEC`/equivalent connection
  path where portable support is practical.

### C-003 [P3]: No live-tailnet acceptance run

- **Where:** Sprint 118 model-free fixture boundary
- **Failure mode:** environment evidence
- **Evidence:** deterministic fake LocalAPI exercises capability/version,
  identity, CAS, preservation, lifecycle ordering, and failure classification.
- **Why it matters:** it cannot validate a real daemon, ACL policy, MagicDNS,
  certificate issuance, or remote reachability.
- **Disposition:** retain INT-0008 AC-9 and an authorized live-tailnet run as
  future acceptance; do not infer it from the fixture.

### C-004 [P3]: Upstream identity and Serve ETag are not atomic

- **Where:** status/config/status identity sandwiches surrounding Serve POST
- **Failure mode:** upstream concurrency boundary
- **Evidence:** tests prove pre/post identity drift fails closed, never reports
  Ready, attempts only scoped compensation, and retains journals when
  unresolved.
- **Why it matters:** Tailscale does not bind StableNodeID/FQDN into the
  Serve-config ETag. A profile switch between the final pre-POST status and the
  POST can theoretically target a different profile with the same config ETag.
- **Disposition:** document that profiles must not change during up/down and
  preserve the current compensating fail-closed behavior.

### C-005 [P3]: Multi-ancestor diagnostics do not mirror exact precedence

- **Where:** cleanup route-shadow projection in `tailscale_serve.rs`
- **Failure mode:** diagnostic precision only
- **Evidence:** all four effective pinned ancestors are detected and retained,
  but if several coexist the reported string follows JSON map iteration rather
  than the upstream matcher order.
- **Why it matters:** cleanup remains blocked and journals remain safe, but the
  operator may be told about `/` before the more-specific `/_ferric` or slash
  alias that wins first.
- **Disposition:** retain as a nonblocking diagnostic refinement with a future
  multi-ancestor precedence test; do not weaken the current fail-closed hold.

## Locked-plan deviation

The finalized plan's CLI-specific T-11801-E03, T-11804-E01, T-11805-E01, and
T-11805-E03 mechanisms are superseded by the direct LocalAPI correction. The
old CLI argv/probe/fixture test names were not executed. This deviation is
recorded in the test report and post-Loop review; the locked plan was not
rewritten.

The frozen T-11802-E02 fault table also names local registration-path resolution
failure. That row was not injected: the path resolver is not yet behind a safe
deterministic effect seam. The other pre-mutation rows passed, while T-11806
retains this descriptive-row gap before exhaustive fault coverage can be
claimed.

The frozen `cargo test -p ferric-cli --doc` command exited 1 with `error: no
library targets found in package ferric-cli`. The applicable supplemental
workspace doc command passed. The all-feature aarch64 attempt was blocked by
the absent external `aarch64-linux-gnu-gcc` toolchain after the default-feature
Ferric aarch64 check passed. None of these qualifications is promoted into a
product pass.

## Confidence

`proceed-with-P3-caveats`

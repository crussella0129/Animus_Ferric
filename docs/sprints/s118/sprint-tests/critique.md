# Test Critique — Sprint 118

## Review result

The mandatory post-evidence adversarial pass examined every Sprint 118 phase,
and parallel independent code/security reviews examined final tested code head
`7633f8c0675664e51c8a4e88e4aaafe0d20880e9`. The complete sprint re-entered
Loop three times: first for the unsafe CLI mutation boundary, second for the
post-evidence code and provenance findings, and third for CI portability and
lifecycle-harness defects. Final re-review found no remaining P0, P1, or P2
issue. The following P3 limitations are retained rather than overstated as
closed.

## Resolved third-Loop findings

- **P2 cfg/dead-code matrix gap:** PR run `33385435515` showed that
  lifecycle-only TCP endpoint symbols were compiled but unused in default and
  `backend-openai` Clippy configurations. Commit `2f976dc` narrowed the four
  ownership cfgs to `feature = "lifecycle-fixture"`; default,
  `backend-openai`, and lifecycle-feature Clippy then passed.
- **P2 PID-1 zombie/reaping gap:** the same run's isolated Linux lifecycle job
  passed 3/5 because the Rust harness was namespace PID 1 and did not reap an
  adopted detached fixture child. Commit `2f976dc` kept an unprivileged
  `/bin/sh` as PID 1 and serialized the harness beneath it. Both operating
  systems subsequently passed 5/5, while the deterministic production
  fail-closed classifier test remained green.
- **P2 hard-cleanup gap:** review of the green `2f976dc` head found that the
  credential transition could clear the `PDEATHSIG` installed by
  `unshare --kill-child=SIGKILL`. Commit `a4bf920` added
  `setpriv --pdeathsig keep`; a bounded parent-SIGKILL proof observed the
  namespace child exit with its parent.
- **P2 workflow quoting gap:** `a4bf920` runs `33388127765` and `33388132395`
  failed before the harness because an apostrophe in a comment terminated the
  outer single-quoted shell program and exposed an unset `$1`. Commit
  `7633f8c` removed the apostrophe; exact local wrapper execution passed 5/5,
  and independent shell review found no remaining P0-P2 defect.

The exact-head push and PR runs `33388704624` and `33388709925` both passed.
No lifecycle-infrastructure result is used to close T-11707's ordinary-host
Linux authority limitation.

### Retained CI-wrapper qualifications [P3]

- The nested shell remains maintenance-sensitive: a future literal apostrophe
  inside the outer single-quoted program would reopen the quoting defect.
  Exact-script execution and CI cover the present bytes; future wrapper edits
  require the same shell-focused review.
- Ubuntu's installed `unshare` does not provide the newer signal-forwarding
  option used on some util-linux versions and does not transparently forward
  ordinary termination while waiting. The wrapper therefore relies on
  `--kill-child=SIGKILL`, restored `PDEATHSIG`, and the bounded CI job cleanup
  for hard process-tree termination.
- Orphan reaping relies on the Ubuntu runner's `/bin/sh` behavior as namespace
  PID 1. Both exact local execution and final Ubuntu CI passed; a runner shell
  change requires requalification.
- If the harness itself terminates by signal, the shell exposes a nonzero
  `128 + signal` status rather than preserving richer signal identity. This
  still fails the job and cannot create a false pass.

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

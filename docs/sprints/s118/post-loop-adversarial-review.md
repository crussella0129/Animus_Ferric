# Sprint 118 Post-Loop Adversarial Review

## Decision

**Proceed with the existing Sprint 118 PR #105 from final tested code head
`7633f8c0675664e51c8a4e88e4aaafe0d20880e9`; do not open another PR.** The
first adversarial pass did not accept the completed-looking CLI implementation.
It reopened Research, Build, Test, and Loop because a separate Tailscale CLI
process could not support the ownership claim under hostile concurrency or
ambiguous completion. The direct LocalAPI ETag/`If-Match` correction was then
reopened by the requested post-evidence five-phase audit for further P2s. A
third Loop re-entry followed after PR CI exposed cfg-specific dead code and a
PID-1 zombie in the isolated Linux lifecycle wrapper. Those findings were
corrected through `2f976dc`, `a4bf920`, and `7633f8c`; exact-head push run
`33388704624` and PR run `33388709925` both passed.

The residual P3 limits are explicit: no successful native Linux-UDS or Windows
pipe exchange, a narrow Unix two-step `CLOEXEC` fork window, no live-tailnet
run, no upstream atomic binding between status identity and Serve-config ETag,
and diagnostic—not safety—imprecision when several effective ancestors coexist.
The lifecycle CI wrapper also retains P3-only shell portability and maintenance
qualifications recorded in the Test critique; none weakens product authority or
turns a failing harness into a pass.

## Five-phase integrity audit

| Phase | Required work | Evidence inspected | Adversarial conclusion |
|---|---|---|---|
| Research | inspect intent, prior lifecycle authority, current code, Tailscale semantics, risks, and environment limits | [research report](sprint-research/research-report.md), INT-0008, Sprint 117 evidence, pinned Tailscale v1.102.2 Serve CLI/config/LocalAPI/backend sources | occurred; the first CLI conclusion was later falsified by Loop, so Research was reopened and corrected to direct LocalAPI, identity sandwiches, exact ETag/CAS, failure classification, scaffold hazards, and platform transports |
| Plan | define bounded intent traceability, EARS outcomes, build order, and tests before implementation | finalized [build plan](sprint-plans/build-plan.md), finalized [test plan](sprint-plans/test-plan.md), clean [plan critique](sprint-plans/critique.md) | occurred; locked plans remain immutable, while mechanism-specific CLI clauses are explicitly superseded below rather than silently rewritten |
| Build | implement durable exact ownership, mutation, status, cleanup, docs, and deterministic fixture | substantive LocalAPI commit `625fbba`, adversarial corrections `9ff40c0`, prior tested head `d5e61b7`, cfg/reaper correction `2f976dc`, hard-cleanup correction `a4bf920`, final wrapper correction and tested head `7633f8c`, `tailscale_localapi.rs`, `tailscale_serve.rs`, server lifecycle/registration/resolution, operator docs, lifecycle fixture, CI wrapper | occurred with three Loop re-entries: the initial CLI build was rejected, the completed-looking LocalAPI build was hardened for pinned ancestor matching/future-version status/fresh-CAS schema races, and PR CI then drove cfg and isolated-Linux-wrapper corrections |
| Test | run named unit/composition/integration/E2E, regressions, portability checks, and preserve qualifications | [test report](sprint-tests/test-report.md), [unit](sprint-tests/unit-tests.md), [integration](sprint-tests/integration-tests.md), [E2E](sprint-tests/e2e-tests.md), [critique](sprint-tests/critique.md), push/PR CI runs cited below | occurred; the prior full matrix retained LocalAPI 19/19, Serve 17/17, server filter 84/84, lifecycle 5/5, frozen aggregate 55+2, and workspace gates, while the corrected exact Linux wrapper passed 5/5 locally and final exact-head push/PR CI runs `33388704624`/`33388709925` passed |
| Loop | critique the implementation and evidence, correct material findings, rerun gates, then independently review again | this record, corrected code heads, supersession table, mandatory post-evidence phase audit, PR CI chronology, parallel code/security reviews | occurred three times; each pass surfaced material defects before merge, all corrections were requalified, the final exact-head CI pair is green, and final Book/phase re-review found no remaining P0-P2 issue |

All five phases therefore occurred in order, with an explicit Research/Build/Test
re-entry when Loop invalidated the original mechanism and a later Build/Test
re-entry when PR CI invalidated the completed-looking evidence head. The PR is
not justified by the earlier CLI head, `d5e61b7`, `85f5e5b`, or the green but
superseded `2f976dc` head.

## Adversarial finding that reopened the sprint

The locked plan intentionally deferred hostile takeover of the unguessable
path during the CLI compare/`off` window. The extra Loop pass found that this
was incompatible with the broader exact-ownership claim and that the CLI
process boundary also obscured mutation completion: Ferric could not prove that
a failed or timed-out child had not committed, could not atomically condition
the mutation on the exact configuration it validated, and could not provide a
test ledger for raw ETag/body authority.

Further review expanded the gap into concrete failure classes:

- a Tailscale profile/FQDN change could cross identity boundaries between
  inspection, mutation, and cleanup;
- a parent, descendant, or trailing-slash route could shadow the token even
  after its exact handler was removed;
- blindly removing HTTPS/TCP/Web scaffolding could damage preexisting,
  concurrent, Funnel, foreground, or future-version state;
- a future compatible daemon could require recovery while unknown numeric JSON
  lexemes made reserialization unsafe;
- Windows pending overlapped I/O required bounded cancellation and poisoned
  non-reuse, not stack-owned state after timeout;
- a generic environment endpoint override would create a production bypass if
  it were accepted by the ordinary `ferric` binary;
- a fake CLI argv log could not prove exact HTTP ordering, ETag/body equality,
  no mutation retry, or same-session identity binding.

The sprint was therefore reopened before PR creation.

## Mandatory post-evidence audit and second Loop re-entry

After the initial LocalAPI correction and evidence appeared complete, the
user-requested final adversarial pass checked Research, Plan, Build, Test, and
Loop as one chain. It blocked PR creation again and required these corrections:

- ordinary status now uses the pinned cleanup projection to expose effective
  `/`, `//`, `/_ferric`, and `/_ferric/` ancestors after exact-handler removal,
  but refuses future-version cleanup observations as proof of Active;
- pinned cleanup strictly validates its initial observation, fresh pre-CAS body,
  absent-only body, and post-CAS body, preventing an unknown-schema race from
  being reserialized under 1.102.2 semantics;
- doctor and onboarding wording now describe the exact LocalAPI
  capability/version contract rather than the retired CLI boundary;
- T-11510 now has a durable umbrella completion transition, while T-11810 owns
  native transport/atomic `CLOEXEC` proof;
- the frozen T-11802-E02 local-path-resolution row is explicitly recorded as
  descriptive and deferred to T-11806 instead of being silently claimed; and
- test counts, exact command outcomes, critique, intent, work state, and sprint
  metadata were reconciled to the final tested head.

This second re-entry established `d5e61b7` as the locally accepted behavior
head rather than the earlier completed-looking `625fbba` evidence state. It is
preserved as historical provenance, not the final PR-tested head.

## PR CI audit and third Loop re-entry

The evidence commit `85f5e5b` opened existing PR #105 and triggered both push
run `33385391918` and PR run `33385435515`; both failed. Default-feature
Clippy on Ubuntu and Windows, plus backend-openai Clippy on Ubuntu, found the
same three lifecycle-fixture-only items compiled into ordinary test targets:
`TEST_TCP_ENDPOINT_ENV`, `Endpoint::Invalid`, and
`parse_test_tcp_endpoint`. The earlier all-feature Clippy result remained
historically true but did not cover that cfg matrix.

The same `85f5e5b` Linux lifecycle job passed 3/5. The Rust test harness had
been executed as PID 1 inside the isolated PID/network/proc namespace, so it
could not reap adopted exited managed children. A later test then encountered
the resulting PID-1 zombie as an unreadable `/proc/<pid>/fd` peer and correctly
failed closed. This was fixture infrastructure, not evidence that ordinary-host
Linux lifecycle authority had become complete; T-11707 remains open.

Commit `2f976dc` narrowed the test endpoint constant, invalid endpoint variant,
parser, and match arm to the `lifecycle-fixture` feature. It also kept an
unprivileged shell as namespace PID 1 to reap adopted children while running
the Rust harness as its child. Push run `33387648205` and PR run `33387653011`
both passed at that exact head. Review nevertheless superseded it because
`setpriv` can clear the parent-death signal installed by
`unshare --kill-child=SIGKILL`, weakening hard cleanup if the outer namespace
process died.

Commit `a4bf920` added `setpriv --pdeathsig keep`, but its explanatory comment
contained the apostrophe in `unshare's` inside an outer single-quoted
`/bin/sh -ceu` program. That prematurely terminated the program string, so
push run `33388127765` and PR run `33388132395` failed before lifecycle
qualification. Commit `7633f8c` removed the quote-breaking apostrophe without
changing the intended wrapper behavior.

The exact corrected wrapper—isolated PID/network/proc namespaces with
`--kill-child=SIGKILL`, an unprivileged PID-1 reaper shell,
`setpriv --pdeathsig keep`, capability removal, and the serialized Rust
harness as a child—passed 5/5 locally. At final tested code head
`7633f8c0675664e51c8a4e88e4aaafe0d20880e9`, push run `33388704624` and PR
run `33388709925` both completed successfully. This is the third Loop re-entry
and the final remote qualification; no second Sprint 118 PR was created.

## Supplemental correction research

The correction retained the pinned primary sources in the research report and
additionally inspected the v1.102.2 LocalAPI status identity fields,
canonical FQDN/certificate-domain representation, capability/version contract,
raw Serve-config ETag behavior, and conventional Linux/Windows transport
endpoints. Those supplemental findings are preserved here as the basis for the
following constraints:

- normal operations pin Tailscale core 1.102.2 and capability 142;
- authority uses StableNodeID plus canonical FQDN and HTTPS readiness, not a
  mutable display hostname;
- each authoritative configuration observation is enclosed by same-session
  status reads;
- the Serve ETag must equal the SHA-256 of the exact raw body;
- apply/off sends at most one POST with exact `If-Match` and never retries after
  bytes may have escaped;
- HTTP 412 is definite no-mutation, while post-send I/O/protocol/daemon failure
  is indeterminate;
- Linux uses the conventional Unix-domain socket, Windows uses the protected
  named pipe, and macOS remains explicitly unsupported rather than guessed.

Pinned primary-source provenance:

- [LocalAPI request handlers and response contract](https://github.com/tailscale/tailscale/blob/v1.102.2/ipn/localapi/localapi.go)
- [Serve-config GET/POST, ETag, `If-Match`, and 412 handling](https://github.com/tailscale/tailscale/blob/v1.102.2/ipn/localapi/serve.go)
- [status and Self-node schema](https://github.com/tailscale/tailscale/blob/v1.102.2/ipn/ipnstate/ipnstate.go)
- [capability version definitions](https://github.com/tailscale/tailscale/blob/v1.102.2/tailcfg/tailcfg.go)
- [Serve ETag/CAS and effective handler matching](https://github.com/tailscale/tailscale/blob/v1.102.2/ipn/ipnlocal/serve.go)
- [platform socket defaults, including the protected Windows pipe](https://github.com/tailscale/tailscale/blob/v1.102.2/paths/paths.go)
- [generic native connection contract](https://github.com/tailscale/tailscale/blob/v1.102.2/safesocket/safesocket.go)
  plus the platform-specific [Unix-domain-socket transport](https://github.com/tailscale/tailscale/blob/v1.102.2/safesocket/unixsocket.go)
  and [Windows named-pipe transport](https://github.com/tailscale/tailscale/blob/v1.102.2/safesocket/pipe_windows.go)

No external artifact was copied into the repository. The source findings are
design evidence; the focused and lifecycle suites remain the acceptance
evidence.

## Corrected build boundary

The final implementation:

1. exposes only the LocalAPI status and Serve-config requests needed for the
   lifecycle, with bounded headers/bodies/deadlines and duplicate-safe JSON;
2. journals ownership before the first CAS with an explicit
   `apply_confirmed=false` phase, then promotes unchanged mirrors only after an
   exact postapply observation;
3. binds publication to StableNodeID, FQDN, HTTPS authority, exact handler and
   target, raw configuration provenance, and scaffold provenance;
4. lets cleanup follow a same StableNodeID across an FQDN rename but never
   crosses to another stable node; it targets the journaled old FQDN;
5. removes only the exact handler, preserves unrelated and shared scaffolding,
   retains journals for route shadows or any unresolved effective route, and
   permits compatible-version cleanup only when no unknown numeric lexeme would
   change;
6. treats proxy cleanup and exact-child cleanup as independently authorized,
   while deleting journals only after both resolve;
7. confines the loopback TCP LocalAPI seam to the separately named
   `ferric-lifecycle-test` binary; the ordinary production binary ignores it.

## Locked-plan supersession record

The finalized plan files say `DO NOT EDIT` and were not changed. The following
mechanism clauses are retained as historical provenance but superseded by the
correction:

| Locked clause | Frozen mechanism | Corrected mechanism and evidence |
|---|---|---|
| T-11801-E03 | fixed Tailscale CLI apply/off argv and no shell/reset API | closed LocalAPI method/path surface, raw-body SHA-256 ETag, one exact `If-Match` POST, no retry, typed 412/no-mutation and post-send/indeterminate outcomes; `exact_request_headers_and_cas_etag`, `serve_cas_412_is_typed_no_mutation`, `post_send_timeout_is_indeterminate` |
| T-11804-E01 | bounded read-only `whoami --json` and `serve status --json` subprocesses | bounded LocalAPI status/config/status session with pinned capability/version and StableNodeID/FQDN/HTTPS binding; `doctor_tailscale_is_bounded_and_read_only`, `status_binding_uses_stable_id_and_https_capability`, `session_reuses_one_connection_for_status_serve_status` |
| T-11805-E01 | real Ferric against fake engine and fake Tailscale CLI executables | real `ferric-lifecycle-test` against fake engine and stateful fake HTTP LocalAPI; `tailscale_localapi_lifecycle_preserves_unrelated_state` |
| T-11805-E03 | fake CLI argv ledger | exact LocalAPI connection/method/path/header/body/ETag/CAS ledger, exactly two POSTs, no retry or broad route, and both journals present before each POST; `tailscale_localapi_log_contains_no_broad_mutation_or_retry` plus `ordinary_ferric_ignores_lifecycle_localapi_override` |

The CLI-specific test names are not reported as executed. T-11801-E01/E02's
planned CLI status transport is likewise replaced by stricter LocalAPI
projection/parsing tests; their absent/exact/ambiguous semantic outcomes remain
covered.

## Corrected Test evidence

The full locally qualified behavior matrix at historical code head
`d5e61b7f951ca838ea2aed7cefaa2468282bb164` was:

- LocalAPI focused suite: 19 passed, 0 failed;
- Serve focused suite: 17 passed, 0 failed;
- server substring filter: 84 passed, 0 failed, 0 ignored, including six
  `api::server::tests`;
- serialized lifecycle fixture: 5 passed, 0 failed, 0 ignored;
- exact frozen `tailscale_` aggregate: 55 unit plus 2 lifecycle tests passed;
- workspace all-target/all-feature tests: passed outside the restricted sandbox
  after the restricted run could not qualify a nested Python child;
- workspace all-target/all-feature Clippy with warnings denied: passed;
- formatting, applicable workspace docs, and both server help smokes: passed;
- default-feature `ferric` aarch64 Linux check: passed;
- all-feature aarch64 check: blocked only at `ring` by the missing external
  `aarch64-linux-gnu-gcc` cross compiler, with no Ferric diagnostic;
- protected Sprint 114 acquisition artifact: unchanged, unstaged, SHA-256
  `8ECF94878E7AD745AEA28A9365AF58EE111C80B26D21A15A0F434EDB2BEB75DB`.

The third Loop re-entry then supplied configuration and platform qualification
that the all-feature local matrix had not exercised:

- `85f5e5b` push/PR CI runs `33385391918`/`33385435515` failed the default and
  backend-openai cfg matrix and the isolated Linux lifecycle wrapper as
  described above;
- `2f976dc` push/PR CI runs `33387648205`/`33387653011` passed, but review
  superseded that head because its credential transition did not explicitly
  retain the namespace parent-death cleanup signal;
- `a4bf920` push/PR CI runs `33388127765`/`33388132395` failed because an
  apostrophe broke the outer single-quoted wrapper program;
- the corrected wrapper passed 5/5 locally at `7633f8c`; and
- final exact-head push run `33388704624` and PR run `33388709925` both passed
  at `7633f8c0675664e51c8a4e88e4aaafe0d20880e9`.

The frozen `cargo test -p ferric-cli --doc` command exited 1 with `error: no
library targets found in package ferric-cli`; the workspace doc surface is the
applicable supplemental gate and passed. No live daemon, tailnet, ACL,
certificate, or remote reachability claim is inferred from model-free evidence.

## Final independent review

Parallel independent reviewers performed findings-first code/security and
five-phase Book passes after the first two corrections and test rerun. The
third Loop re-entry then used failed and green CI, plus follow-up wrapper
review, to close the cfg dead code, PID-1 zombie, parent-death-signal, and shell
quoting defects before merge. The exact final code head is remotely green, and
independent final code/workflow and evidence/phase passes after reconciliation
found no remaining P0-P2 issue. The remaining P3 caveats are unchanged:

1. **Positive native transport E2E:** the TCP seam proves protocol/lifecycle;
   Windows adds native negative timeout/cancellation/poisoning and Linux
   compiles the UDS implementation, but neither has a successful native
   pipe/UDS exchange.
2. **Unix `CLOEXEC`:** the descriptor is marked close-on-exec in a second step,
   leaving a narrow fork/exec inheritance window for arbitrary future
   multithreaded callers.
3. **Live tailnet:** model-free fixtures do not establish ACL, MagicDNS,
   certificate, or remote reachability acceptance.
4. **Identity/ETag atomicity:** the upstream APIs do not atomically bind status
   identity into the Serve ETag; sandwiches detect drift and force scoped
   compensation/evidence retention but cannot erase the theoretical switch
   immediately before POST.
5. **Multi-ancestor diagnostic precedence:** any effective ancestor blocks
   cleanup and retains journals, but the displayed route follows map iteration
   rather than the upstream matcher's exact precedence when several coexist.

These are follow-up and operator-boundary items, not concealed acceptance
claims. The [Test critique](sprint-tests/critique.md) records their disposition.
That critique also retains the CI-only nested-quoting, signal-forwarding,
runner-shell reaping, and signaled-exit-status qualifications discovered in the
third Loop re-entry.

## PR gate

The sprint satisfies the requested Research → Plan → Build → Test → Loop
sequence and three Loop re-entries. Existing PR #105 is the only Sprint 118 PR;
do not open another. Merge remains authorized only after the root workflow
confirms that:

- `origin/main..dev` contains Sprint 118 commits only;
- the evidence commit does not stage the protected Sprint 114 artifact;
- pushed `origin/dev` equals local `dev`; and
- exactly one Sprint 118 PR targets `main` from `dev`;
- final tested code head `7633f8c` retains successful push/PR runs
  `33388704624`/`33388709925`; and
- the evidence-only reconciliation and final adversarial Book pass are clean.

The owner remains the only merge authority.

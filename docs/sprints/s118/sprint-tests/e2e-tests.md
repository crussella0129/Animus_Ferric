# Sprint 118 End-to-End Test Results

- **Tested code head:** `7633f8c0675664e51c8a4e88e4aaafe0d20880e9`
- **Command:** `cargo test -p ferric-cli --test server_lifecycle_fixture --all-features -- --test-threads=1 --nocapture`
- **Result:** 5 passed, 0 failed, 0 ignored.

## Model-free real-process lifecycle

`tailscale_localapi_lifecycle_preserves_unrelated_state` passed. The real
feature-gated `ferric-lifecycle-test` binary ran Tailscale doctor, up, status,
down, and repeated down against an isolated fake engine plus a stateful
loopback HTTP/1.1 LocalAPI. The fixture proved:

- normal operations sent capability 142 and accepted the pinned Tailscale
  1.102.2 identity;
- every Serve-config read was enclosed by same-connection
  status/config/status observations;
- the complete lifecycle sent exactly two configuration POSTs: one apply CAS
  and one cleanup CAS, with no mutation retry or broad endpoint;
- each POST carried the exact ETag of the raw configuration body in
  `If-Match`, and byte-identical local/global ownership journals existed before
  the POST;
- the apply journal was unconfirmed, the cleanup journal was confirmed, and
  both recorded the exact StableNodeID, FQDN, mount, target, remote base,
  scaffold provenance, and pre-state digest;
- the unrelated same-host handler and unrelated `Services` data survived,
  while the final Serve configuration was semantically equal to its initial
  state;
- status rendered the exact tokenized remote `/v1` base; down stopped the exact
  process only after scoped proxy cleanup, removed both journals when resolved,
  and repeated down converged without another mutation.

`tailscale_localapi_log_contains_no_broad_mutation_or_retry` independently
checks the closed request ledger: only `GET /localapi/v0/status?peers=false`,
`GET /localapi/v0/serve-config`, and exact
`POST /localapi/v0/serve-config` requests are accepted. POST headers and bodies
must match the one-CAS contract.

`ordinary_ferric_ignores_lifecycle_localapi_override` passed and recorded zero
fake-server requests from the ordinary production binary, proving the loopback
TCP endpoint seam is confined to the separately named lifecycle-test target.
The preexisting ordinary model-free lifecycle and legacy-adoption/down E2Es
also remained green.

These replace the frozen fake-Tailscale-executable and argv-log mechanisms in
T-11805-E01/E03. The old
`tailscale_cli_lifecycle_preserves_unrelated_state` and
`tailscale_command_log_contains_no_broad_mutation` names were not executed and
are not claimed as passes.

## Linux fixture correction and exact-head CI

The first PR lifecycle run at evidence head `85f5e5b` was not accepted. Run
[33385435515](https://github.com/crussella0129/Animus_Ferric/actions/runs/33385435515)
passed the Windows lifecycle job but the isolated Ubuntu job passed only 3/5.
The Rust test harness was namespace PID 1, so an adopted detached managed child
remained a zombie between serialized tests and poisoned a later `/proc`
listener-owner query. The production classifier correctly failed closed; the
fixture topology, not that authority rule, required correction. The same run
also exposed the lifecycle-only TCP endpoint's default/backend Clippy cfg gap.

Commit `2f976dc` narrowed that cfg boundary and kept an unprivileged `/bin/sh`
as namespace PID 1 to reap adopted children while the Rust harness ran beneath
it. Push run
[33387648205](https://github.com/crussella0129/Animus_Ferric/actions/runs/33387648205)
and PR run
[33387653011](https://github.com/crussella0129/Animus_Ferric/actions/runs/33387653011)
completed successfully with both operating-system lifecycle jobs green, but
review superseded that head because the credential transition could clear the
parent-death signal behind `unshare --kill-child=SIGKILL`.

Commit `a4bf920` added `setpriv --pdeathsig keep`. Its push run
[33388127765](https://github.com/crussella0129/Animus_Ferric/actions/runs/33388127765)
and PR run
[33388132395](https://github.com/crussella0129/Animus_Ferric/actions/runs/33388132395)
kept the Windows lifecycle job and every non-lifecycle job green, but the
Ubuntu lifecycle job failed before the harness: an apostrophe in a comment
closed the outer single-quoted shell program and exposed an unset `$1` to
outer Bash with `set -u`.

Commit `7633f8c` removed that quote hazard. The exact isolated Linux wrapper
passed 5/5 locally. Final push run
[33388704624](https://github.com/crussella0129/Animus_Ferric/actions/runs/33388704624)
and PR run
[33388709925](https://github.com/crussella0129/Animus_Ferric/actions/runs/33388709925)
both completed successfully at
`7633f8c0675664e51c8a4e88e4aaafe0d20880e9`, including green serialized
lifecycle jobs on Ubuntu and Windows.

## Operator smoke and protected evidence

- `cargo run -p ferric-cli --bin ferric -- server up --help`: passed.
- `cargo run -p ferric-cli --bin ferric -- server doctor --help`: passed.
- The protected Sprint 114 acquisition artifact remained unstaged and retained
  SHA-256
  `8ECF94878E7AD745AEA28A9365AF58EE111C80B26D21A15A0F434EDB2BEB75DB`.

## Explicit evidence limits

This is deterministic model-free evidence, not a live-tailnet acceptance run.
It does not prove real ACL, MagicDNS, certificate issuance, remote reachability,
or a successful production-native transport exchange. The positive lifecycle
uses the isolated loopback TCP test seam. Windows adds a real named-pipe
negative pending-I/O timeout/cancellation/poisoned-nonreuse case, while Linux
compiles the Unix-domain-socket implementation; neither platform has a
successful native pipe/UDS fake-or-real-daemon exchange in this sprint.

The isolated Linux namespace intentionally makes all relevant peers visible
to one capability-free runner identity. It does not prove ordinary-host Linux
lifecycle authority when unrelated `/proc/<pid>/fd` peers are unreadable or
shared. T-11707 remains open; production teardown continues to fail closed
when ownership visibility is incomplete.

The upstream status and Serve-config endpoints do not provide one atomic
StableNodeID/FQDN/ETag authority. Same-session sandwiches detect profile drift
before and after a CAS and force compensation/evidence retention, but cannot
remove the theoretical switch between the last status read and POST. Operators
must not switch Tailscale profiles during Ferric lifecycle commands. macOS
LocalAPI discovery and full AC-8 parity remain outside Sprint 118.

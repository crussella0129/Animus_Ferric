# Sprint 118 Integration Test Results

- **Tested code head:** `7633f8c0675664e51c8a4e88e4aaafe0d20880e9`
- **Primary command:** `cargo test -p ferric-cli --all-features server::tests`
- **Result:** 84 passed, 0 failed, 0 ignored. The substring filter includes
  six `api::server::tests` alongside the server composition tests.

## Deterministic lifecycle composition

`tailscale_fault_seam_clause_matrix` and its named constituent matrices passed.
The scripted effect ledgers prove ordering and authority rather than only a
successful terminal state:

- exact child health/identity/listener checks and byte-identical unconfirmed
  journals precede the first LocalAPI mutation;
- preapply and postapply status/config/status observations remain in one
  bounded LocalAPI session and bind StableNodeID, FQDN, HTTPS authority,
  capability/version, the exact handler projection, and raw-body ETag;
- only an exact postapply observation authorizes mirrored journal promotion to
  `apply_confirmed=true` and final Ready publication;
- HTTP 412 is definite no-mutation, while any post-send transport/protocol
  failure is indeterminate, is not retried, and retains recovery evidence;
- identity switches, same-node renames, listener/child drift, phase-torn
  mirrors, registration-revision races, replacement targets, descendant or
  alias or ancestor route shadows, future routing semantics, and cleanup errors
  each reach the specified fail-closed state;
- external proxy cleanup is attempted before independently authorized exact
  process teardown, and registrations disappear only after both resources are
  resolved;
- legacy boolean-only ownership remains non-authorizing and triggers no
  LocalAPI mutation or process signal.

`mirrored_tailscale_provenance_conflicts_block_before_effects` exercises each
scaffold-provenance disagreement across local/global mirrors.
`tailscale_identity_races_never_publish_or_cross_profile_cleanup` proves that a
preapply rename, postapply rename, or StableNodeID switch never becomes Ready
and never authorizes cross-profile removal. A same-StableNodeID rename remains
cleanable through the journaled FQDN even after current HTTPS/certificate
authority is lost.

## Configuration-preservation boundary

The 17-test Serve suite supplies integration-level projection/mutation proof
for pristine and shared configurations. It preserves unrelated handlers,
Services, Funnel/foreground dependencies, preexisting scaffolding, and
concurrently added scaffolding. Descendants, trailing-slash aliases, and the
pinned matcher's effective `/`, `//`, `/_ferric`, or `/_ferric/` ancestors are
retained as route shadows and block resolved cleanup after the exact handler is
absent; an unrelated-host copy of the token is preserved without conferring
authority. Pinned observations and the fresh removal CAS snapshot reject
unknown schema fields rather than reserializing them.

Compatible major-1 version-drift cleanup can remove only the exact recorded
handler and never tears down shared scaffolding. It fails closed if JSON
reserialization would change any numeric lexeme, so future unknown numeric
state is not silently rewritten.

## CI and Linux fixture reconciliation

PR run
[33385435515](https://github.com/crussella0129/Animus_Ferric/actions/runs/33385435515)
at `85f5e5b` rejected the first completed-looking evidence state. Default
Clippy failed on Ubuntu and Windows and `backend-openai` Clippy failed on
Ubuntu because lifecycle-only TCP endpoint items remained present in
non-lifecycle test configurations. The same run's isolated Linux lifecycle
job passed 3/5: running the Rust harness as namespace PID 1 left an adopted
managed child as a zombie, and a later listener-owner query correctly failed
closed rather than treating incomplete `/proc` inspection as authority.

Commit `2f976dc` narrowed the TCP seam's cfg boundary and placed the serialized
harness under an unprivileged `/bin/sh` namespace PID-1 reaper. Push run
[33387648205](https://github.com/crussella0129/Animus_Ferric/actions/runs/33387648205)
and PR run
[33387653011](https://github.com/crussella0129/Animus_Ferric/actions/runs/33387653011)
completed successfully, but the head was superseded when adversarial review
found that the UID/GID transition could clear the `PDEATHSIG` supporting
`unshare --kill-child=SIGKILL`. Commit `a4bf920` restored that contract with
`setpriv --pdeathsig keep`; its push/PR runs `33388127765` and `33388132395`
then failed only the Ubuntu lifecycle job before the harness because an
apostrophe broke the outer shell program's single-quote boundary.

Commit `7633f8c` removed the quote hazard. The exact isolated wrapper passed
5/5 locally, and final push run
[33388704624](https://github.com/crussella0129/Animus_Ferric/actions/runs/33388704624)
and PR run
[33388709925](https://github.com/crussella0129/Animus_Ferric/actions/runs/33388709925)
both completed successfully at the full tested code head. Their Ubuntu and
Windows default and lifecycle jobs, plus the Ubuntu backend and aarch64 jobs,
were all green.

## Regression boundary

The complete workspace all-target/all-feature test command passed outside the
restricted sandbox. The restricted run could not qualify a nested benchmark
Python child; granting ordinary child-process permission to the identical
command produced the authoritative green result. Workspace Clippy with
warnings denied, formatting, applicable doc tests, and help smokes also passed.
Final exact-head CI additionally passed default Clippy and workspace tests on
Ubuntu and Windows, `backend-openai` Clippy on Ubuntu, and
`lifecycle-fixture` Clippy and tests on both operating systems.

The default-feature `ferric` aarch64 Linux check passed. The all-feature cross
check was blocked only when `ring` requested the absent
`aarch64-linux-gnu-gcc` cross compiler; no Ferric source diagnostic occurred.
That environment limitation is not represented as all-feature aarch64
acceptance.

The locked plan's CLI-specific adapter/probe/fixture mechanisms are superseded
by direct LocalAPI evidence, as detailed in
[the post-Loop review](../post-loop-adversarial-review.md). The locked files
remain unchanged, and no obsolete CLI test name is reported as executed.

The isolated Linux namespace makes every relevant peer visible to the
capability-free fixture identity; it is not evidence that ordinary shared-host
Linux can completely enumerate every unrelated `/proc/<pid>/fd` peer.
T-11707 therefore remains open and the production classifier continues to
fail closed for unreadable or shared owners.

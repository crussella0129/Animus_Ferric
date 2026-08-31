# Sprint 118 Integration Test Results

- **Tested code head:** `d5e61b7f951ca838ea2aed7cefaa2468282bb164`
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

## Regression boundary

The complete workspace all-target/all-feature test command passed outside the
restricted sandbox. The restricted run could not qualify a nested benchmark
Python child; granting ordinary child-process permission to the identical
command produced the authoritative green result. Workspace Clippy with
warnings denied, formatting, applicable doc tests, and help smokes also passed.

The default-feature `ferric` aarch64 Linux check passed. The all-feature cross
check was blocked only when `ring` requested the absent
`aarch64-linux-gnu-gcc` cross compiler; no Ferric source diagnostic occurred.
That environment limitation is not represented as all-feature aarch64
acceptance.

The locked plan's CLI-specific adapter/probe/fixture mechanisms are superseded
by direct LocalAPI evidence, as detailed in
[the post-Loop review](../post-loop-adversarial-review.md). The locked files
remain unchanged, and no obsolete CLI test name is reported as executed.

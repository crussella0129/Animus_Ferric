# Sprint 117 Integration Test Evidence

**Intent:** [INT-0008 — Unified local-model workflow](../../../intents/INT-0008-unified-local-model-workflow.md),
AC-3/4/6/7 and enabling evidence toward AC-9.

**Plans:** [finalized build plan](../sprint-plans/build-plan.md) and
[finalized test plan](../sprint-plans/test-plan.md).

## Evidence identity

All commands targeted immutable implementation head
`b679a25ba83069ab849b0f7f2eb8a3269eba10c5`. Local results used Microsoft
Windows NT 10.0.26200.0 x64 and Rust 1.96.0. Linux-native results came from
the ordinary Ubuntu CI job at that exact head. Every row exited 0.

## Registration and lifecycle composition

| Clauses | Exact command | Result |
|---|---|---|
| E01-C/D | `cargo test -p ferric-cli --all-features --locked two_process_lifecycle_interleaving_is_per_path_safe -- --nocapture` | Exactly 1 passed; two real child clients exercised shared-path publication, inventory, adoption, removal, and replacement preservation. |
| E03-A/B/C | `cargo test -p ferric-cli --all-features --locked status_and_discovery_two_scope_matrix -- --nocapture` | Exactly 1 passed; one typed two-scope inventory composed through status, backend, autonomy, doctor, and down. |
| E02-A/B/C, E04-A/B/C/D | `cargo test -p ferric-cli --all-features --locked down_retained_handle_transition_matrix -- --nocapture` | Exactly 1 passed; retained-handle observation, signalling, post-exit listener, and per-alias cleanup transitions stayed ordered. |
| E02-C, E05-A/B | `cargo test -p ferric-cli --all-features --locked up_spawned_child_binding_precedes_readiness -- --nocapture` | Exactly 1 passed; retained binding and pre-publication authority preceded persistence, then post-publication retained identity and exclusive listener authority gated `Ready`. Identity/listener transitions stopped and reaped the retained child before exact compensation. |
| E04-E, E04-A | `cargo test -p ferric-cli --all-features --locked server::tests::legacy_adoption_then_down -- --exact --nocapture` | Exactly 1 passed; adoption did not signal, then down reacquired and stopped the same generation. |
| E05-A/B | `cargo test -p ferric-cli --all-features --locked registration_publication_failure_matrix -- --nocapture` | Exactly 1 passed; the production publisher crossed every scripted persistence/runtime compensation boundary, including the successful post-publication reinspection. |

The exact single-test rerun for `legacy_adoption_then_down` supersedes an
earlier successful short-filter invocation that also selected the similarly
named CLI E2E. No composition row relies on that overmatched result.

## Native platform smokes

| Target | Exact command | Result |
|---|---|---|
| Windows x86_64 | `cargo test -p ferric-cli --all-features --locked windows_retained_process_handle_smoke -- --nocapture` | Exactly 1 passed. E02-A/B/C and E04-A/B exercised a real retained Windows handle, canonical FILETIME token, native listener classification, handle-only terminate/wait, and release. |
| Linux x86_64 on `ubuntu-latest` | `cargo test --workspace` in CI job [99366993856](https://github.com/crussella0129/Animus_Ferric/actions/runs/33351978700/job/99366993856) | The retained log names `server_process::platform::tests::linux_pidfd_process_handle_smoke` as `ok` inside the 250/250 Linux `ferric` unit binary. E02-A/B/C and E04-A/B exercised boot-id/start-ticks identity, pidfd signal/poll, IPv4/IPv6 ownership, inherited-FD sharing, and descriptor release. |

The earlier exact WSL filter belonged to the superseded implementation head
and is not used as final acceptance evidence. The retained ordinary Ubuntu CI
log proves the named native smoke at `b679a25`. The AArch64 lifecycle surface
compile-checked, but no AArch64 runtime result is claimed.

## Suite-level corroboration

- `cargo test -p ferric-cli --all-features --locked` passed 336/336 and
  includes every composition and Windows-native row above.
- `cargo test --workspace --all-features --locked` passed 1,089 with 4
  intentional ignores and no failures.
- CI run
  [33351978700](https://github.com/crussella0129/Animus_Ferric/actions/runs/33351978700)
  passed the ordinary Ubuntu and Windows workspace jobs at the identical head.

# Sprint 117 Integration Test Evidence

**Intent:** [INT-0008 — Unified local-model workflow](../../../intents/INT-0008-unified-local-model-workflow.md),
AC-3/4/6/7 and enabling evidence toward AC-9.

**Plans:** [finalized build plan](../sprint-plans/build-plan.md) and
[finalized test plan](../sprint-plans/test-plan.md).

## Evidence identity

All commands targeted immutable implementation head
`44f36a239f48c4c230e0454535451ecf62e5ffa3`. Local results used Microsoft
Windows NT 10.0.26200.0 x64 and Rust 1.96.0. Linux-native results came from
the ordinary Ubuntu CI job at that exact head. Every row exited 0.

## Registration and lifecycle composition

| Clauses | Exact command | Result |
|---|---|---|
| E01-C/D | `cargo test -p ferric-cli --all-features --locked two_process_lifecycle_interleaving_is_per_path_safe -- --nocapture` | 1 passed in 0.40s; two real child clients exercised shared-path publication, inventory, adoption, removal, and replacement preservation. |
| E03-A/B/C | `cargo test -p ferric-cli --all-features --locked status_and_discovery_two_scope_matrix -- --nocapture` | 1 passed in 0.04s; one typed two-scope inventory composed through status, backend, autonomy, doctor, and down. |
| E02-A/B/C, E04-A/B/C/D | `cargo test -p ferric-cli --all-features --locked down_retained_handle_transition_matrix -- --nocapture` | 1 passed in 0.02s; retained-handle observation, signalling, post-exit listener, and per-alias cleanup transitions stayed ordered. |
| E02-C, E05-A | `cargo test -p ferric-cli --all-features --locked up_spawned_child_binding_precedes_readiness -- --nocapture` | 1 passed in 0.01s; retained binding preceded readiness and publication. |
| E04-E, E04-A | `cargo test -p ferric-cli --all-features --locked server::tests::legacy_adoption_then_down -- --exact --nocapture` | 1 passed in 0.03s; adoption did not signal, then down reacquired and stopped the same generation. |
| E05-A/B | `cargo test -p ferric-cli --all-features --locked registration_publication_failure_matrix -- --nocapture` | 1 passed in 0.38s; the production publisher crossed every scripted persistence/runtime compensation boundary. |

The exact single-test rerun for `legacy_adoption_then_down` supersedes an
earlier successful short-filter invocation that also selected the similarly
named CLI E2E. No composition row relies on that overmatched result.

## Native platform smokes

| Target | Exact command | Result |
|---|---|---|
| Windows x86_64 | `cargo test -p ferric-cli --all-features --locked windows_retained_process_handle_smoke -- --nocapture` | 1 passed in 0.07s. E02-A/B/C and E04-A/B exercised a real retained Windows handle, canonical FILETIME token, native listener classification, handle-only terminate/wait, and release. |
| Linux x86_64 on `ubuntu-latest` | `cargo test --workspace` in CI job [99351509302](https://github.com/crussella0129/Animus_Ferric/actions/runs/33346491895/job/99351509302) | The retained log names `server_process::platform::tests::linux_pidfd_process_handle_smoke` as `ok` inside the 250/250 Linux `ferric` unit binary. E02-A/B/C and E04-A/B exercised boot-id/start-ticks identity, pidfd signal/poll, IPv4/IPv6 ownership, inherited-FD sharing, and descriptor release. |

The earlier exact WSL filter belonged to the superseded implementation head
and is not used as final acceptance evidence. The retained ordinary Ubuntu CI
log proves the named native smoke at `44f36a2`. The AArch64 lifecycle surface
compile-checked, but no AArch64 runtime result is claimed.

## Suite-level corroboration

- `cargo test -p ferric-cli --all-features --locked` passed 336/336 and
  includes every composition and Windows-native row above.
- `cargo test --workspace --all-features --locked` passed 1,089 with 4
  intentional ignores and no failures.
- CI run
  [33346491895](https://github.com/crussella0129/Animus_Ferric/actions/runs/33346491895)
  passed the ordinary Ubuntu and Windows workspace jobs at the identical head.

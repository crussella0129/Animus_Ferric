# Sprint 131 — Test report

## New unit test

`restricted_peers_are_skipped_but_every_listener_must_be_the_engines`
covers four cases:

- restricted peers, PID 1 included, are skipped;
- a squatter's own listener on the port is still refused;
- a readable process sharing the engine's socket still gives
  `OwnedByOther`;
- an unreadable engine is uninspectable.

`linux_uninspectable_shared_listener_owner_is_not_exclusive` still checks
that non-permission read failures fail closed.

## The ordinary-user scenario

The CLI test binary was run as `nobody`, for whom root's processes are
unreadable. All 58 `startup::tests`, `human::enabled::tests` and
`live_budget_tests` passed. Before this change the same check refused every
launch.

## The sandbox (T-12810, closed)

| Suite | Before | After |
|---|---|---|
| `ferric-cli` lib tests in this sandbox | 19 failures | 1 failure: `linux_pidfd_process_handle_smoke` |
| `server_lifecycle_fixture` (including `model_free_server_lifecycle_fixture_e2e`) | the end-to-end test failed | all 3 pass |

**Why the sandbox failed before:** PID 1 here cannot be listed even by
root, so the check refused every launch.

**Why one test still fails:** this sandbox has no IPv6 ("Address family not
supported"), and the smoke test's IPv6 modes need it. CI runners have IPv6
and pass it.

## Gates

- `cargo fmt --all -- --check` passes.
- Workspace clippy and lifecycle-fixture clippy pass with `-D warnings`.
- `cargo test --workspace --no-fail-fast` fails only the IPv6 smoke test
  above.
- CI result: see the PR.

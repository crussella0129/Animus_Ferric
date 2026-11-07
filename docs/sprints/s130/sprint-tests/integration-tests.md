# Sprint 130 — Integration tests

- **ferric-cli lifecycle fixture:** `retired_registrations_block_cli_without_signal_or_delete`
  runs the real binary against a live fixture. It checks that `status`,
  `down` and `doctor` refuse retired records without signalling or deleting
  anything, and that manual cleanup then works. It replaces
  `legacy_adoption_then_down_cli_e2e`. The two fake-LocalAPI Tailscale tests
  were removed with Tailscale Serve.
- **ferric-loop:** `pre_sprint_113_trace_is_refused`. Fixtures for
  current-behaviour replay tests were rewritten into the
  proposal/commit format.
- **tests/cli.rs:** the planner value is rejected before any trace or
  workspace effect.
  `query_without_backend_errors` was removed with the backend-free build.

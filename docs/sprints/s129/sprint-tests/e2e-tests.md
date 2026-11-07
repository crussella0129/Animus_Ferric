# Sprint 129 — End-to-end tests

No new end-to-end flows. The sprint changes structure, not behaviour, so the
existing end-to-end coverage is the gate:

- `tests/cli.rs` and `human_cli.rs` drive the `ferric` binary;
- `bench_budget.rs` and `query_output_budget.rs` run a real HTTP fake
  server;
- the lifecycle fixture runs on both CI operating systems.

`tools/e2e_test.ps1` is now live-model only. Its mock branch repeated
`mock_query_end_to_end`.

# Sprint 129 — Integration tests

- **ferric-loop:**
  - D-8: `a_truncated_turn_does_not_spend_the_empty_completion_nudge_on_resume`,
    which also replays a pre-fix checkpoint;
  - `an_unconstrained_truncated_turn_spends_the_empty_completion_nudge`.
  - The loop area was additionally checked by recording every trace event
    the suite writes. Across 145 sessions and 4,416 events, the output
    matched the pre-refactor tree once timestamps, PIDs and hashes were
    masked.
- **ferric-cli:**
  - `scaffold_refuses_a_symlinked_target` (D-9);
  - one shared support module (`tests/support/mod.rs`): an isolated
    command, bounded output, `help()` and one fake OpenAI server;
  - the four identical-run mock-query tests are merged into
    `mock_query_end_to_end`, with every assertion kept.
- **ferric-valve:**
  - `malformed_utf8_is_a_stream_error`;
  - `record_only_forwards_malformed_utf8_unchanged`;
  - the 502 server tests now also assert the receipt's error class.
- **ferric-process:**
  - `scope_cleanup_closes_descendant_listener`;
  - `parent_death_signal_kills_a_leaf_child`;
  - `upstream_owner_death_drains_watched_scopes`.

  These take over the scenarios that ferric-cli tested through a parallel
  harness.
- **Lifecycle fixture:**
  - `server_lifecycle_fixture` passes on GitHub's Ubuntu and Windows
    runners.
  - In this sandbox its four process-owning cases fail for the same reason
    as before the sprint ("PID 1 is uninspectable"), with no new failures.

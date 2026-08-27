Finalized - DO NOT EDIT

# Sprint 112 Test Plan

## Recovery contract

- Round-trip every new event and parse representative pre-sprint traces.
- Resume after interrupt, provider failure, max turns, and guard pause.
- Resume a resumed session after a second interruption.
- Reject completed sessions and canonical workspace mismatches.
- Inject crashes before dispatch, after mutation, after tool result, and after
  `TurnCommitted`; assert no duplicate effect and no silent divergent history.
- Restore guard/compaction state or explicitly record any deferred state.

## Clarification and verification

- Parse and dispatch `request_user_input` in every supported protocol.
- Assert `NeedsInput` is non-success and carries question/context/options plus a
  continuation identifier.
- Resume with an answer and prove it is visible as a pinned goal amendment.
- Reject unknown checks, altered argv, over-time checks, and stale passing
  evidence after a later write.
- Accept completion only after the configured check passes after the final
  mutation; preserve legacy behavior when no check is configured.

## Harness

- Prove user config/model profiles cannot contaminate a run.
- Prove spec `max_turns` reaches the query process.
- Fill child pipes past their buffer and prove no deadlock.
- Require exact successful terminal events and executable artifact checks.
- Retain run/trial/provenance/trace fields and fail visibly on malformed JSONL.
- Report each task/tool with Wilson intervals; never infer promotion from a
  pooled rate.

## Quality and live acceptance

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- Focused crate tests after every commit and `cargo test --workspace
  --all-features` at sprint end.
- Three process-cold real-server lifecycles with independent PID/listener/HTTP
  identity and clean teardown.
- Ten retained live demo-task repetitions and the versioned internal matrix, or
  an explicit timed report of the completed sample if hardware runtime exceeds
  the sprint window.
- Trace verification must remain side-effect-free.


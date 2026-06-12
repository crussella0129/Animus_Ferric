# Sprint 0 Integration Tests — Results

Run 2026-06-10, local Windows: **ALL PASS**.

### `crates/ferric-tools/tests/guarded_traced_execution.rs`
- `guarded_traced_execution` — temp workspace + registry + JsonlSink: write_file → read_file traced as ToolCall/ToolResult pairs with full output, durations, monotonic seq; denied `.git/config` write traced as error, handler never ran, nothing created on disk. **pass**

### `crates/ferric-provider/tests/mock_loop_skeleton.rs`
- `mock_loop_skeleton` — MockProvider scripted (tool-call completion, then text completion); minimal test-harness loop: tools_for_policy(NANO) → descriptors, constraint attached to every request, registry dispatch, truncated result fed back as `Role::Tool` message, loop ends on text-only completion within `policy.max_turns`. Asserts: final answer, 2 provider requests, constraint present end-to-end, tool result fed back with correct id/content, trace = [start, call, result, end]. **pass**
- This test is the structural template for the s1 L0-smoke E2E. It is deliberately test-only — the production loop crate is an s1 decision.

### `crates/ferric-core/tests/tier_table_snapshot.rs`
- `tier_table_snapshot` — `policy_for` pinned over the real 5-GGUF fleet (1B Nano / 7B Small / 7B Small / 8B Small / 14B Medium) with full policy-field assertions. Any tier-table change is now a reviewed diff. **pass**

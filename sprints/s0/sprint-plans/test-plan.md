Finalized - DO NOT EDIT

# Sprint 0 Test Plan — Animus Ferric Foundations

## Unit Tests

### T-001 unit tests
- (build gates serve as the tests) `cargo build`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` all exit 0 at workspace root.
- Stubs: none.

### T-002 unit tests
- `message_roundtrip_json`: Message with tool_call → serialize → deserialize → equal.
- `toolcall_args_polymorphic`: args as `"x"`, `[1]`, `{"a":1}` → all deserialize Ok.
- Stubs: none.

### T-003 unit tests
- `policy_for_is_deterministic`: same profile twice → identical RunPolicy.
- `nano_tier_boundaries`: params_b 0.5 / 3.9 / 4.0 / 13.1 → NANO / NANO / SMALL / MEDIUM.
- `nano_policy_shape`: 1B profile → ConstrainedJson, uses_planner, max_tools ≤ NANO ceiling.
- `measured_level_overrides_params`: 7B profile + measured L1 → NANO-grade policy (downgrade direction).
- `measured_level_upgrade`: 1B profile + measured L4 → SMALL-grade policy (upgrade direction).
- Stubs: none (pure function).

### T-004 unit tests
- `jsonl_roundtrip_all_event_types`: write one of each Event → read back equal.
- `reader_tolerates_unknown_event`: hand-written JSONL with `"type":"FUTURE_EVENT"` → yields Unknown preserving raw JSON, no error.
- `seq_monotonic_per_session`: 100 writes → seq 0..99.
- `tool_result_full_output`: ToolResult with long output → full text present in file.
- Stubs: tempfile dirs.

### T-005 unit tests
- `mock_replays_script_in_order`: script [A,B] → complete()=A then B then ScriptExhausted error.
- `constraint_recorded_by_mock`: request with JsonSchema constraint → mock's last_request contains it.
- `provider_is_dyn_compatible`: `Box<dyn Provider>` compiles and dispatches.
- Stubs: MockProvider itself; async via `futures_executor::block_on`.

### T-006 unit tests
- `rejects_dotdot_escape`: `../x` → BoundaryViolation.
- `rejects_absolute_outside`: absolute path outside root → BoundaryViolation.
- `rejects_prefix_collision`: root `project`, candidate `project-evil/x` → BoundaryViolation.
- `rejects_symlink_escape` (`#[cfg(unix)]`): in-workspace symlink → outside target → BoundaryViolation.
- `accepts_nested_valid_path`: `src/a/b.rs` → Ok(canonical) on both OSes.
- Stubs: tempfile workspaces.

### T-007 unit tests
- `denies_sensitive_paths`: Write to `.git/config`- and `.ssh/`-shaped paths → Deny with reason.
- `allows_plain_read`: Read of ordinary in-workspace file → Allow.
- `denylist_is_const`: deny lists exposed only as consts (no setter API).
- Stubs: tempfile workspaces.

### T-008 unit tests
- `execute_blocks_on_deny`: tool targeting denied path → handler not invoked (flag tool proves it), Denied result.
- `output_truncation_preserves_full`: 1MB dummy output → full = 1MB, model copy ≤ limit.
- `tools_for_policy_sorted_and_capped`: NANO policy + 10 registered tools → ≤ max_tools, alphabetical order.
- Stubs: flag-setting dummy tool.

### T-009 unit tests
- `write_then_read_roundtrip`: tempdir workspace → content equality.
- `tools_refuse_outside_workspace`: absolute outside path → error, file absent.
- `list_dir_deterministic_order`: 5 files → two calls, identical sorted order.
- Stubs: tempfile workspaces.

### T-010 unit tests
- `trace_cat_renders_unknown`: mixed JSONL (known + unknown events) → exit 0, unknown labeled.
- `version_flag`: `ferric --version` → prints version, exit 0.
- Stubs: tempfile JSONL fixtures (invoke binary via `CARGO_BIN_EXE`).

### T-011 / T-012 / T-013 unit tests
- T-011/T-013 verified by CI itself: green matrix run (fmt/clippy/test win+linux, aarch64 check) on the pushed main, conclusion checked as a separate step; `git remote -v` shows origin = crussella0129/Animus_Ferric.
- T-012 verified by inspection: `decisions.md` contains nine dated ADR entries (001–009).

## Integration Tests

### Security + tools + trace integration (`crates/ferric-tools/tests/guarded_traced_execution.rs`)
- `test_guarded_traced_execution`: temp workspace + registry + JsonlSink → execute `write_file` then `read_file` → trace contains ToolCall/ToolResult pairs with full output, durations, monotonic seq; then a denied call → deny traced, target file untouched.

### Provider + tools + trace integration (`crates/ferric-provider/tests/mock_loop_skeleton.rs`)
- `test_mock_loop_skeleton`: MockProvider scripted to emit a ToolCall(read_file) completion → minimal test-harness loop dispatches through registry → truncated result fed back as message → second scripted completion ends loop → complete trace on disk. Proves all s0 interfaces compose into the eventual ReAct loop shape. (Test-only harness — the production loop crate is an s1 decision.)

### Scale-function snapshot (`crates/ferric-core/tests/tier_table_snapshot.rs`)
- `test_tier_table_snapshot`: `policy_for` over the real 5-GGUF fleet profiles (1B / 7B / 7B / 8B / 14B) → snapshot assertion so any tier-table change is a visible reviewed diff.

## End-to-End Tests
- **Status:** not-yet-possible (by design — s0 has no inference backend).
- Unlocked by: sprint s1 — first real `Provider` backend (mistral.rs in-process or llama-server HTTP) + ported L0–L6 benchmark harness + Llama-3.2-1B (771 MB) as cheapest fleet model. First E2E: **L0 smoke — one real-GGUF run produces a valid trace and a correct file edit.** `mock_loop_skeleton` is the structural template. The CPU-first baseline decision (user-locked) makes this unlock hardware-independent: L0 smoke runs CPU-only, so GPU vendor/VRAM unknowns cannot block it.

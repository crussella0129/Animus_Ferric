Finalized - DO NOT EDIT

# Sprint 2 Build Plan — Prompts, Unified Action Grammar, Calibration

## Schema Tree
- Sprint Goal: Versioned prompts (oovra), grammar-impossible malformed actions, measured tier calibration
  - Foundation
    - T-201: Workspace members + s2 dependency set (oovra rev-pin, toml, regex, preserve_order)
    - T-202: ActionProtocol + RunPolicy.max_output_tokens
    - T-203: PolicySelected + PromptComposed trace events
    - T-204: Completion.truncated from finish_reason
    - T-205: move_path + make_dir tools
  - Unified action grammar
    - T-206: Grammar module (schema generator + action parser)
    - T-207: Loop ActionProtocol integration
    - T-208: Truncated/malformed action handling
  - Prompts
    - T-209: ferric-prompt crate + element library + compose
    - T-210: Query wiring (--protocol, --prompts-dir, policy max_tokens)
  - Benchmark harness
    - T-211: Spec model + embedded TOML L0–L6
    - T-212: Runner (spawn-self, timeout)
    - T-213: Trace verification + results row
    - T-214: Calibration (results.jsonl + model_profiles.json)
    - T-215: ferric bench subcommand
  - Gates & records
    - T-216: Real-model gates (smoke ×2 protocols, calibration sweep) + ADR-015..019

## Execution Sequence

### T-201: Wire workspace members and the s2 dependency set.
- **Touches:** `Cargo.toml`, `crates/ferric-prompt/{Cargo.toml,src/lib.rs}` (stub), `crates/ferric-bench/{Cargo.toml,src/lib.rs}` (stub), `Cargo.lock`
- **Depends on:** (none)
- **Success criterion (EARS):**
  - **WHEN** `cargo check --workspace` runs with default features, **THEN** it **SHALL** compile with neither mistralrs nor tokio in the graph and Cargo.lock **SHALL** pin oovra at rev `378abea6552f74ee7a8e4d9ce74418d6457ad002`.
  - **WHEN** the aarch64 check runs, **THEN** the workspace including ferric-prompt and ferric-bench **SHALL** type-check.
  - **WHEN** a serde_json object built by insertion is serialized, **THEN** key order **SHALL** be insertion order (`preserve_order` active workspace-wide).
- **Notes:** oovra rev-pinned to main (lib API verified at that rev: Library::load/get, render::compose/render_text, PromptElement, OovraError; a git rev is immutable so later branch movement cannot drift it — C-004). The acceptance test for the pin is T-209 compiling and calling that API. toml 0.8 (matches oovra transitive). regex + tempfile-as-regular-dep for ferric-bench. ferric-prompt deps: oovra, ferric-core, thiserror. ferric-bench deps: serde, serde_json, toml, regex, tempfile, thiserror, ferric-core, ferric-trace. ADR-016 records the allowlist growth.

### T-202: Add ActionProtocol to ferric-core and max_output_tokens to RunPolicy.
- **Touches:** `crates/ferric-core/src/scale.rs`, `crates/ferric-core/src/lib.rs`, `crates/ferric-core/tests/tier_table_snapshot.rs`
- **Depends on:** (none)
- **Success criterion (EARS):**
  - **WHEN** `policy_for` runs per tier, **THEN** `max_output_tokens` **SHALL** be NANO 512 / SMALL 768 / MEDIUM 1024 / LARGE 1536 / XL 2048 / ULTRA 2048, pinned by the snapshot test.
  - **WHEN** `ActionProtocol` round-trips through serde, **THEN** it **SHALL** serialize as `"native_tools"` / `"unified_grammar"`.
- **Notes:** Seeds leave headroom over the largest expected single action (~450-token write_file through L4) while capping a 1B's worst-case turn (~1 min CPU). ADR-018.

### T-203: Add PolicySelected and PromptComposed trace events + render arms.
- **Touches:** `crates/ferric-trace/src/event.rs`, `crates/ferric-trace/src/lib.rs` (tests), `crates/ferric-cli/src/trace_cmd.rs`
- **Depends on:** (none)
- **Success criterion (EARS):**
  - **WHEN** `PolicySelected { tier, protocol, max_turns, max_tools, prompt_budget_tokens, max_output_tokens }` and `PromptComposed { output_id, output_version, composed_of }` round-trip through the reader, **THEN** they **SHALL** parse as Known events with `TRACE_SCHEMA_VERSION` remaining 1.
  - **WHEN** `trace cat` renders them, **THEN** it **SHALL** produce one human line each with no `[unknown event]` fallback.

### T-204: Plumb truncation through Completion and the mistralrs backend.
- **Touches:** `crates/ferric-provider/src/types.rs`, `crates/ferric-provider/src/mistralrs.rs`, `crates/ferric-provider/src/mock.rs`, touched Completion literals in ferric-loop tests + query.rs mock script
- **Depends on:** (none)
- **Success criterion (EARS):**
  - **WHEN** the backend maps a choice with `finish_reason == "length"`, **THEN** `Completion.truncated` **SHALL** be true; for `"stop"`/`"tool_calls"`/`"canceled"` it **SHALL** be false (free-function unit test).
  - **WHEN** MockProvider scripts a completion, **THEN** tests **SHALL** be able to set `truncated` (default false).
- **Notes:** mistralrs 0.8.1 `Choice.finish_reason: String` (response.rs:88, source-verified).

### T-205: Add move_path and make_dir builtin tools (NANO, Write).
- **Touches:** `crates/ferric-tools/src/builtin/{move_path.rs,make_dir.rs,mod.rs}`, tests
- **Depends on:** (none)
- **Success criterion (EARS):**
  - **WHEN** `move_path {from, to}` declares targets, **THEN** `target_paths` **SHALL** include BOTH endpoints and **WHEN** either escapes the workspace, **THEN** the call **SHALL** be denied with no rename performed.
  - **WHEN** `move_path` runs on an existing file or directory, **THEN** it **SHALL** rename it; a missing source **SHALL** be an is_error result, not a panic.
  - **WHEN** `make_dir {path}` runs, **THEN** it **SHALL** create the directory with parents, idempotent on existing dirs.
- **Notes:** Unblocks bench L1/L2; lineage NANO ops (North Star: simple ops 100% accurate).

### T-206: Implement the grammar module: schema generator + action parser.
- **Touches:** `crates/ferric-loop/src/grammar.rs`, `crates/ferric-loop/src/lib.rs`
- **Depends on:** T-201
- **Success criterion (EARS):**
  - **WHEN** `action_schema(&[ToolDescriptor])` runs over N descriptors + the terminator, **THEN** it **SHALL** emit `{"x-guidance":{"whitespace_flexible":false},"type":"object","anyOf":[N+1 branches]}` with per-branch `{"properties":{"tool":{"const":name},"args":<input_schema + additionalProperties:false>},"required":["tool","args"],"additionalProperties":false}` and the string `"oneOf"` **SHALL** appear nowhere (llguidance 1.7.6 rejects oneOf).
  - **WHEN** any branch is serialized, **THEN** within that branch's `properties` object the keys **SHALL** appear in insertion order with `"tool"` first, then `"args"` (early branch commitment, pinned vs preserve_order regression), **AND** branches **SHALL** appear in the deterministic offered-tools order (registry-sorted, terminator last) so schema output is reproducible (C-013).
  - **WHEN** `parse_action` receives completion text, **THEN** a valid `{"tool","args"}` document **SHALL** yield a synthesized ToolCall (id `g-<turn>-0`) and anything else **SHALL** yield a typed error, never a panic.

### T-207: Integrate ActionProtocol into the loop.
- **Touches:** `crates/ferric-loop/src/run.rs`, `crates/ferric-loop/src/lib.rs`, ferric-loop tests
- **Depends on:** T-202, T-203, T-204, T-206
- **Success criterion (EARS):**
  - **WHEN** the loop starts, **THEN** it **SHALL** emit `PolicySelected` (and `PromptComposed` when RunArgs carries lineage) immediately after SessionStart; **WHEN** protocol is UnifiedGrammar, **THEN** every request **SHALL** carry `constraint: Some(JsonSchema(action_schema))` with tools EMPTY, tracing `ConstraintApplied { kind: "json_schema" }`.
  - **WHEN** a grammar-mode completion parses to an Action, **THEN** it **SHALL** route through the SAME dispatch path as native tool calls (terminator interception, repetition guard, permission events, ToolCall/ToolResult tracing identical) and the result **SHALL** return as a user-role message `[tool_result for <tool>] <text>`.
  - **WHEN** protocol is NativeTools, **THEN** behavior **SHALL** be byte-identical to s1 (existing tests pass with only Completion-literal updates).
- **Notes:** ActionProtocol is DEFINED in ferric-core (T-202); T-207 imports it (C-001). RunArgs gains `protocol: ActionProtocol` + `prompt_lineage: Option<Vec<(String, String)>>` (plain id+version tuples — no ferric-prompt dep in ferric-loop; matches PromptComposed.composed_of shape, C-011). `select_protocol(&RunPolicy, &Capabilities, Option<ActionProtocol>)` lives in ferric-loop and gets its own unit tests (C-001). Grammar mode has no FinalText path by design: every completion must be an action; valid JSON that is not `{tool,args}`-shaped is REJECTED (typed error → empty-completion path), never treated as final text (C-012).

### T-208: Handle truncated and malformed grammar actions.
- **Touches:** `crates/ferric-loop/src/{run.rs,outcome.rs}`
- **Depends on:** T-207
- **Success criterion (EARS):**
  - **WHEN** a grammar-mode completion arrives truncated, **THEN** the loop **SHALL NOT** parse or dispatch it, **SHALL** nudge once, and **WHEN** a second truncation occurs, **THEN** it **SHALL** stop with `StopReason::TruncatedAction` → `SessionEnd { reason: "truncated_action" }`.
  - **WHEN** a non-truncated grammar completion fails parse_action, **THEN** it **SHALL** reuse the empty-completion nudge-once path and stop with `"empty_completion"`.
- **Notes:** Truncation = budget signal; parse failure = backend-integrity signal (unreachable under a real grammar; reachable via mocks). Explicitly adds `StopReason::TruncatedAction` to outcome.rs + its `as_str()` mapping `"truncated_action"` (C-002). Mock truncation injection: MockProvider completions carry `truncated` settable per T-204 — that is how the integration tests script this path (C-007); the bench runner never injects truncation (real truncation is exercised only by real-model gates).

### T-209: Create ferric-prompt: element library + recipe matrix + compose_system_prompt.
- **Touches:** `crates/ferric-prompt/src/lib.rs`, repo-root `prompts/*.md` (atoms: role-declaration, workspace-rules, protocol-native-tools, protocol-unified-grammar, terminator-teaching + NANO variants where needed)
- **Depends on:** T-201, T-202
- **Success criterion (EARS):**
  - **WHEN** `compose_system_prompt(&Library, Tier, ActionProtocol)` runs for every tier × protocol pair, **THEN** it **SHALL** return `Ok(ComposedPrompt { text, output_id, output_version, composed_of })` with `composed_of` exactly matching `recipe_for(tier, protocol)`.
  - **WHEN** protocol is UnifiedGrammar, **THEN** the composed text **SHALL** teach the `{"tool","args"}` format incl. task_complete and **SHALL NOT** teach native tool-call syntax (and vice versa).
  - **WHEN** the library root is missing or an element fails to load, **THEN** the error **SHALL** surface as typed Err (caller falls back to DEFAULT_SYSTEM_PROMPT; never silent).

### T-210: Wire query: --protocol, --prompts-dir, policy-driven max_tokens.
- **Touches:** `crates/ferric-cli/src/{query.rs,main.rs}`, `crates/ferric-cli/Cargo.toml`
- **Depends on:** T-207, T-208, T-209
- **Success criterion (EARS):**
  - **WHEN** `--protocol native|grammar` is given, **THEN** it **SHALL** override select_protocol; absent, the policy/capabilities mapping **SHALL** decide (mock advertises constraint support so `--mock --protocol grammar` is model-free).
  - **WHEN** `--prompts-dir` (or FERRIC_PROMPTS_DIR) names a loadable library, **THEN** composed prompt + lineage **SHALL** flow into RunArgs and PromptComposed **SHALL** appear in the trace; failing/absent, the run **SHALL** proceed on DEFAULT_SYSTEM_PROMPT with a Note event.
  - **WHEN** sampling is built, **THEN** `SamplingParams.max_tokens` (existing field, no provider type change) **SHALL** be set from `policy.max_output_tokens` at the query.rs construction site (C-003).

### T-211: Define the bench spec model and port L0–L6 as embedded TOML.
- **Touches:** `crates/ferric-bench/src/spec.rs`, `crates/ferric-bench/specs/l0.toml..l6.toml`
- **Depends on:** T-201, T-205
- **Success criterion (EARS):**
  - **WHEN** all seven embedded specs parse, **THEN** each **SHALL** yield level, prompt, setup_files, expectations (file|dir|missing + content_regex), expected/any_of/forbidden tools, max_iterations, wall_clock_timeout_s, optional post_verify — with deny_unknown_fields rejecting typos.
  - **WHEN** L1/L2 specs name tools, **THEN** they **SHALL** use Ferric names (move_path, make_dir) and L0's forbidden set **SHALL** include write_file, move_path, make_dir.

### T-212: Implement the runner: workspace materialization + spawn-self subprocess with timeout.
- **Touches:** `crates/ferric-bench/src/runner.rs`
- **Depends on:** T-211
- **Success criterion (EARS):**
  - **WHEN** a run starts, **THEN** setup_files **SHALL** materialize into a fresh tempdir and the child **SHALL** spawn as `<ferric-bin> query <prompt> --workspace <tmp> ...` with `<ferric-bin>` defaulting to `std::env::current_exe()` (child is always `query` — bench recursion structurally impossible) and `--ferric-bin` as override.
  - **WHEN** the child exceeds wall_clock_timeout_s, **THEN** it **SHALL** be killed (std try_wait poll loop) and the row marked timed_out.
  - **WHEN** --keep-workspace is set, **THEN** the tempdir **SHALL** be preserved and printed.

### T-213: Implement trace verification and the results row.
- **Touches:** `crates/ferric-bench/src/{verify.rs,results.rs}`
- **Depends on:** T-212, T-203
- **Success criterion (EARS):**
  - **WHEN** a child trace is parsed, **THEN** the row **SHALL** derive iterations (count TurnStart), tokens (TurnEnd sums), wall (ts_ms), terminator (SessionEnd.reason), tier/protocol (PolicySelected), tool_calls, repetition_guard_fires, task_complete summary + failure-admission scan — with plan_steps/steps_executed null (flagged, not faked).
  - **WHEN** verdicts compose, **THEN** completed **SHALL** equal `!timed_out ∧ exit==0 ∧ expectations_ok ∧ tools_ok ∧ post_verify_ok ∧ terminator ∈ {task_complete, final_text}`.

### T-214: Implement calibration: results.jsonl append + model_profiles.json.
- **Touches:** `crates/ferric-bench/src/calibrate.rs`
- **Depends on:** T-213
- **Success criterion (EARS):**
  - **WHEN** a sweep finishes, **THEN** each row **SHALL** append (never truncate) to `<results-dir>/results.jsonl` and measured_level (highest completed level) **SHALL** write to `<results-dir>/model_profiles.json` keyed by model file + variant/protocol + timestamp.
  - **WHEN** a row is written, **THEN** it **SHALL** record both tier_from_params and tier_from_measured.

### T-215: Add the ferric bench subcommand.
- **Touches:** `crates/ferric-cli/src/{main.rs,bench_cmd.rs}`, `crates/ferric-cli/Cargo.toml`
- **Depends on:** T-214, T-210
- **Success criterion (EARS):**
  - **WHEN** `ferric bench` runs with --level/--model-dir/--model-file/--params-b/--ctx/--variant/--protocol/--prompts-dir/--keep-workspace/--results-dir/--specs-dir/--ferric-bin/--mock, **THEN** it **SHALL** execute selected levels in order and exit nonzero iff any requested level fails.
  - **WHEN** --mock is set, **THEN** the run **SHALL** be fully model-free (CI-runnable harness self-test).
- **Notes:** Warn when cfg!(debug_assertions) — spawn-self from a debug binary spawns debug children (s1 ~1 tok/s lesson).

### T-216: Real-model gates + ADR records.
- **Touches:** `crates/ferric-cli/tests/l0_smoke.rs` (shared assertion helper + second #[ignore] fn `l0_smoke_grammar`), `decisions.md` (ADR-015..019), `agent-tasks/`
- **Depends on:** all
- **Success criterion (EARS):**
  - **WHEN** the smoke suite runs locally against the 1B GGUF in release, **THEN** BOTH protocol variants **SHALL** pass the eight s1-style assertions plus PolicySelected presence, with terminator reason ∈ {task_complete, final_text} for both variants — the grammar's effect on terminator behavior is MEASURED by the sweep, not pre-asserted by the gate (C-010: avoids assuming the conclusion).
  - **WHEN** the calibration sweep runs (1B, L0–L4, both protocols), **THEN** results.jsonl + model_profiles.json **SHALL** be committed as the sprint's empirical record (ADR-009), including the task_complete-rate comparison between protocols; Qwen-7B results are informational and non-blocking (C-010).

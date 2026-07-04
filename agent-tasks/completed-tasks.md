# Completed Tasks Log (Append-Only)

## T-001 (sprint 0)
- **Description:** Create the Cargo workspace with six empty-but-compiling `ferric-*` crates, pinned toolchain, and lint config.
- **Completed:** 2026-06-10T16:05:00Z
- **Files modified:** Cargo.toml, rust-toolchain.toml, README.md, .gitignore, crates/ferric-{core,trace,provider,guard,tools,cli}/Cargo.toml, crates/*/src/lib.rs, crates/ferric-cli/src/main.rs
- **Commit:** 013e0e8

## T-002 (sprint 0)
- **Description:** Define shared vocabulary types Message, Role, ToolCall, FerricError in ferric-core.
- **Completed:** 2026-06-10T16:12:00Z
- **Files modified:** crates/ferric-core/src/lib.rs, crates/ferric-core/src/message.rs, crates/ferric-core/src/error.rs
- **Commit:** 724475e

## T-003 (sprint 0)
- **Description:** Implement the deterministic scale function (ModelProfile, Tier, RunPolicy, Protocol, tier table, pure policy_for) with bidirectional measured-level override and fleet snapshot test.
- **Completed:** 2026-06-10T16:25:00Z
- **Files modified:** crates/ferric-core/src/scale.rs, crates/ferric-core/src/lib.rs, crates/ferric-core/tests/tier_table_snapshot.rs
- **Commit:** 57d23f3

## T-004 (sprint 0)
- **Description:** Build ferric-trace: versioned TraceEvent, flush-per-event JsonlSink, unknown-event-tolerant TraceReader.
- **Completed:** 2026-06-10T16:40:00Z
- **Files modified:** crates/ferric-trace/src/lib.rs, crates/ferric-trace/src/event.rs, crates/ferric-trace/src/sink.rs, crates/ferric-trace/src/reader.rs
- **Commit:** d16de53

## T-005 (sprint 0)
- **Description:** Define the async dyn-compatible Provider trait with Constraint plumbing (JsonSchema/Regex/Lark) and a deterministic scripted MockProvider that records requests.
- **Completed:** 2026-06-10T16:55:00Z
- **Files modified:** crates/ferric-provider/src/lib.rs, crates/ferric-provider/src/traits.rs, crates/ferric-provider/src/types.rs, crates/ferric-provider/src/mock.rs
- **Commit:** 40dced1

## T-006 (sprint 0)
- **Description:** Implement the symlink-safe, prefix-collision-proof workspace boundary in ferric-guard (component-wise canonical containment).
- **Completed:** 2026-06-10T17:05:00Z
- **Files modified:** crates/ferric-guard/src/lib.rs, crates/ferric-guard/src/workspace.rs
- **Commit:** 0c1b6fd

## T-007 (sprint 0)
- **Description:** Add the hardcoded permission checker (Read/Write/Execute, machine-readable deny reasons) and compile-time deny lists.
- **Completed:** 2026-06-10T17:15:00Z
- **Files modified:** crates/ferric-guard/src/checker.rs, crates/ferric-guard/src/denylist.rs, crates/ferric-guard/src/lib.rs
- **Commit:** 382ff6b

## T-008 (sprint 0)
- **Description:** Build the Tool trait, ToolSpec, and registry with a single execute chokepoint (pre-handler guard check, timing, full/truncated output split, sorted+capped tools_for_policy).
- **Completed:** 2026-06-10T17:30:00Z
- **Files modified:** crates/ferric-tools/src/lib.rs, crates/ferric-tools/src/spec.rs, crates/ferric-tools/src/registry.rs
- **Commit:** 7542e2d

## T-009 (sprint 0)
- **Description:** Implement builtin file tools read_file, write_file, list_dir resolving through the workspace boundary, with registry-driven tests.
- **Completed:** 2026-06-10T17:45:00Z
- **Files modified:** crates/ferric-tools/src/builtin/{mod.rs,read_file.rs,write_file.rs,list_dir.rs}, crates/ferric-tools/src/lib.rs, crates/ferric-tools/tests/builtin_file_tools.rs
- **Commit:** 6250637

## T-010 (sprint 0)
- **Description:** Build the ferric CLI stub: --version and trace cat derived view (unknown events labeled, never crash).
- **Completed:** 2026-06-10T17:55:00Z
- **Files modified:** crates/ferric-cli/src/main.rs, crates/ferric-cli/tests/cli.rs
- **Commit:** 17a80ca

## T-011 (sprint 0)
- **Description:** Add GitHub Actions CI: fmt/clippy/test on windows+ubuntu plus aarch64-unknown-linux-gnu check gate. Full gate (incl. aarch64 cross-check) verified locally first.
- **Completed:** 2026-06-10T18:05:00Z
- **Files modified:** .github/workflows/ci.yml
- **Commit:** 4386cad

## T-012 (sprint 0)
- **Description:** Record ADRs 001–009 (dated) in decisions.md.
- **Completed:** 2026-06-10T18:15:00Z
- **Files modified:** decisions.md
- **Commit:** 93a4659

## T-013 (sprint 0)
- **Description:** Create the public GitHub repo crussella0129/Animus_Ferric and push main; CI run 27301488990 conclusion=success on head 93a4659 (verified via gh run list).
- **Completed:** 2026-06-10T19:50:00Z
- **Files modified:** git remote config only
- **Commit:** 2a55547

## T-101 (sprint 1)
- **Description:** Extend trace vocabulary with TurnStart, TurnEnd (completion text + token counts), PromptAssembled, ConstraintApplied, RepetitionGuard, PermissionCheck; trace cat render arms; s0-format compatibility test.
- **Completed:** 2026-06-11T02:30:00Z
- **Files modified:** crates/ferric-trace/src/event.rs, crates/ferric-trace/src/lib.rs, crates/ferric-cli/src/main.rs
- **Commit:** 4ab0a0f

## T-102 (sprint 1)
- **Description:** Registry chokepoint surfaces per-target CheckRecords on Completed/Denied; `.ferric` added to denied write segments (trace self-protection).
- **Completed:** 2026-06-11T02:45:00Z
- **Files modified:** crates/ferric-tools/src/registry.rs, crates/ferric-tools/src/lib.rs, crates/ferric-guard/src/denylist.rs, crates/ferric-tools/tests/guarded_traced_execution.rs, crates/ferric-provider/tests/mock_loop_skeleton.rs
- **Commit:** 78aa53b

## T-103 (sprint 1)
- **Description:** CompletionRequest::validate() (ADR-010 constraint×tools exclusivity) + ProviderError::RetryableBackend variant + is_retryable().
- **Completed:** 2026-06-11T02:55:00Z
- **Files modified:** crates/ferric-provider/src/types.rs
- **Commit:** 26dc9c5

## T-104 (sprint 1)
- **Description:** ferric-loop crate: core turn loop productionizing mock_loop_skeleton (policy budgets, traced stages, denial feedback, empty-completion nudge, best-effort text on MaxTurns). Includes the module scaffolds for T-105..T-107 (terminator/repetition/backoff) which run.rs integrates; their dedicated EARS tests land with their own tasks.
- **Completed:** 2026-06-11T03:30:00Z
- **Files modified:** Cargo.toml, crates/ferric-loop/* (Cargo.toml, src/{lib,run,outcome,terminator,repetition,backoff}.rs, tests/{common/mod.rs,loop_core.rs})
- **Commit:** 415d99a

## T-105 (sprint 1)
- **Description:** task_complete structured terminator EARS tests (terminates without dispatch, mixed-turn ordering, always offered beyond max_tools, malformed summary still terminates, loop ends after terminator). Implementation landed in T-104's scaffold (terminator.rs).
- **Completed:** 2026-06-11T03:50:00Z
- **Files modified:** crates/ferric-loop/tests/terminator_tests.rs, crates/ferric-loop/tests/common/mod.rs
- **Commit:** 0b33b14

## T-106 (sprint 1)
- **Description:** Repetition guard EARS tests (warn → nudge visible to model → stop on third identical set; reset on any change; order change is not a repeat). Implementation landed in T-104's scaffold (repetition.rs).
- **Completed:** 2026-06-11T04:00:00Z
- **Files modified:** crates/ferric-loop/tests/repetition_tests.rs
- **Commit:** 0954168

## T-107 (sprint 1)
- **Description:** Backoff EARS tests via a FlakyProvider (schedule 250/500/1000, exhaustion → provider_error, non-retryable aborts with zero sleeps). Implementation landed in T-104's scaffold (backoff.rs).
- **Completed:** 2026-06-11T04:10:00Z
- **Files modified:** crates/ferric-loop/tests/backoff_tests.rs, crates/ferric-loop/Cargo.toml
- **Commit:** 52fd398

## T-108 (sprint 1)
- **Description:** Workspace deps (mistralrs =0.8.1 feature-gated, tokio feature-gated, clap unconditional) + backend-mistralrs features in provider/cli + CI backend-check job. Verified: default graph contains zero mistralrs/tokio, aarch64 check green, feature-gated clippy compiled mistralrs clean on Windows (exit 0).
- **Completed:** 2026-06-11T04:40:00Z
- **Files modified:** Cargo.toml, Cargo.lock, crates/ferric-provider/{Cargo.toml,src/lib.rs,src/mistralrs.rs}, crates/ferric-cli/Cargo.toml, .github/workflows/ci.yml
- **Commit:** 0672f43

## T-110 (sprint 1)
- **Description:** CLI graduated to clap derive (query flags defined, handler stubbed to T-111; trace cat rendering preserved; usage errors exit non-zero).
- **Completed:** 2026-06-11T04:45:00Z
- **Files modified:** crates/ferric-cli/src/{main.rs,query.rs,trace_cmd.rs}, crates/ferric-cli/tests/cli.rs
- **Commit:** f5a5ad0

## T-109 (sprint 1)
- **Description:** MistralRsProvider (feature-gated): GgufModelBuilder local-dir loading (TokenSource::None, force_cpu, max_num_seqs=2), 1:1 Constraint mapping, native tool calling (ToolChoice::Auto; 0.8.1 has no strict field — drift noted in module docs), usage→token counts, transient/permanent error classification, all mapping in model-free-tested free functions.
- **Completed:** 2026-06-11T05:20:00Z
- **Files modified:** crates/ferric-provider/src/mistralrs.rs
- **Commit:** 68a997f

## T-111 (sprint 1)
- **Description:** ferric query: ModelProfile from flags → policy → loop → stdout + .ferric/trace/<session>.jsonl; --mock path on futures-executor (write_file + task_complete script through the real guard/registry); real path on tokio runtime with MistralRsProvider + HF_HUB_OFFLINE; missing-feature build errors cleanly.
- **Completed:** 2026-06-11T05:50:00Z
- **Files modified:** crates/ferric-cli/src/query.rs, crates/ferric-cli/tests/cli.rs
- **Commit:** 457e493

## T-112 (sprint 1)
- **Description:** L0 smoke E2E — PASSED against real Llama-3.2-1B Q4_K_M (release profile): exit 0, hello.txt exact content, valid monotonic trace, clean final_text termination, write_file call/result/allow-check traced, tools offered, 3 turns / 223 output tokens / 116.9s wall incl. load. Finding: debug-profile inference is ~1 tok/s (37+ min single turn) — --release mandated in the test docs. Observed 1B behavior: described task_complete in prose instead of calling it (lineage failure mode, anticipated by the gate design).
- **Completed:** 2026-06-11T20:20:00Z (local)
- **Files modified:** crates/ferric-cli/tests/l0_smoke.rs
- **Commit:** (see git log for T-112)

## T-113 (sprint 1)
- **Description:** ADR-010..014 recorded (constraint×tools exclusivity; no chat catch-all; MCP-stdio-first; named ownership boundaries + attestation follow-on; pinned capability roadmap + ADR-004 allowlist amendment); backlog rewritten with the s2/s3/s4–s7/s3+ roadmap, user-flagged research leads (tree-sitter rustification re-exam, ownership-graph attestation), updated lineage-fix ledger, and the s2 per-turn output-token budget finding.
- **Completed:** 2026-06-11T20:25:00Z (local)
- **Files modified:** decisions.md, agent-tasks/agent-tasks.md
- **Commit:** (see git log for T-113)

## T-201 (sprint 2)
- **Description:** Workspace members ferric-prompt + ferric-bench (stubs); oovra rev-pinned git dep; toml/regex added; serde_json preserve_order (load-bearing for grammar property order) with pinning test. Default graph verified mistralrs/tokio-free; aarch64 green.
- **Completed:** 2026-06-12 (build phase)
- **Files modified:** Cargo.toml, Cargo.lock, crates/ferric-prompt/*, crates/ferric-bench/*
- **Commit:** c79701a

## T-202 (sprint 2)
- **Description:** ActionProtocol enum (native_tools/unified_grammar) in ferric-core + RunPolicy.max_output_tokens per-tier seeds (512/768/1024/1536/2048/2048), snapshot-pinned.
- **Completed:** 2026-06-12 (build phase)
- **Files modified:** crates/ferric-core/src/{scale.rs,lib.rs}, crates/ferric-core/tests/tier_table_snapshot.rs
- **Commit:** 6f7ba23

## T-203 (sprint 2)
- **Description:** PolicySelected (typed Tier/ActionProtocol + budgets) and PromptComposed (oovra lineage) trace events, round-trip-tested, rendered in trace cat; schema version stays 1.
- **Completed:** 2026-06-12 (build phase)
- **Files modified:** crates/ferric-trace/src/{event.rs,lib.rs}, crates/ferric-cli/src/trace_cmd.rs, crates/ferric-loop/tests/common/mod.rs
- **Commit:** f1a6eda

## T-204 (sprint 2)
- **Description:** Completion.truncated plumbed from mistralrs finish_reason=="length" (is_truncated free-function tested); all Completion literals updated; grammar_completion + truncated_completion test helpers added for T-207/T-208.
- **Completed:** 2026-06-12 (build phase)
- **Files modified:** crates/ferric-provider/src/{types.rs,mistralrs.rs,mock.rs}, crates/ferric-cli/src/query.rs, crates/ferric-provider/tests/mock_loop_skeleton.rs, crates/ferric-loop/tests/common/mod.rs
- **Commit:** 8b8c03c

## T-205 (sprint 2)
- **Description:** move_path (both endpoints boundary-checked via target_paths; missing-source is_error; cross-boundary + .ferric denied with source intact) and make_dir (parents + idempotent) NANO tools; 6 registry-driven tests.
- **Completed:** 2026-06-12 (build phase)
- **Files modified:** crates/ferric-tools/src/builtin/{move_path.rs,make_dir.rs,mod.rs}, crates/ferric-tools/tests/builtin_file_tools.rs
- **Commit:** 5949ff3

## T-206 (sprint 2)
- **Description:** Grammar module: action_schema (anyOf/const-discriminator/x-guidance/additionalProperties:false, tool-before-args insertion order, terminator-last, oneOf-absent — golden-tested) + parse_action (completion text → synthesized ToolCall g-<turn>-0, typed errors for non-JSON/non-object/missing fields/non-action). #[allow(dead_code)] until T-207 wires it.
- **Completed:** 2026-06-12 (build phase)
- **Files modified:** crates/ferric-loop/src/{grammar.rs,lib.rs}
- **Commit:** 1d41f6e

## T-207 (sprint 2)
- **Description:** Loop ActionProtocol integration: PolicySelected/PromptComposed emitted; per-protocol request build (grammar = constraint-only, tools empty — ADR-010 unrepresentable); completion normalized to actions (native tool_calls vs grammar text→parse_action); same dispatch path (terminator/repetition/permission identical); grammar results framed as user-role [tool_result for X]; select_protocol helper. 5 grammar integration tests + native regression intact.
- **Completed:** 2026-06-12 (build phase)
- **Files modified:** crates/ferric-loop/src/{run.rs,lib.rs,protocol.rs}, crates/ferric-loop/tests/{grammar_loop.rs,loop_core.rs,backoff_tests.rs,common/mod.rs}, crates/ferric-cli/src/query.rs
- **Commit:** e8707d5

## T-208 (sprint 2)
- **Description:** StopReason::TruncatedAction; grammar truncation handled as an early branch (don't parse cut-off action; nudge once with "cut off" message, partial NOT added to history; second truncation → truncated_action). Parse-failure stays empty_completion — the two failure modes stay distinguishable. 3 tests.
- **Completed:** 2026-06-13 (build phase)
- **Files modified:** crates/ferric-loop/src/{outcome.rs,run.rs}, crates/ferric-loop/tests/truncation_tests.rs
- **Commit:** c0eebd8

## T-209 (sprint 2)
- **Description:** ferric-prompt crate: 5 prompt atoms in prompts/ (role, workspace-rules, protocol-native-tools, protocol-unified-grammar, terminator-teaching); recipe_for(tier, protocol) with protocol-exclusive teaching; compose_system_prompt via oovra render_text + genealogy (id+version lineage); typed PromptError (Load/Missing/VersionMismatch/Render). 4 tests incl. all-pairs + protocol-exclusivity.
- **Completed:** 2026-06-13 (build phase)
- **Files modified:** crates/ferric-prompt/src/lib.rs, prompts/*.md
- **Commit:** 4819cd2

## T-210 (sprint 2)
- **Description:** query wiring: --protocol native|grammar override (select_protocol with constraint-capable caps), --prompts-dir/FERRIC_PROMPTS_DIR (compose_system_prompt → system_prompt + PromptComposed lineage; Note + DEFAULT fallback on failure), SamplingParams.max_tokens = policy.max_output_tokens, protocol-aware built-in mock script (native vs grammar shape). Query variant boxed (clippy large_enum_variant). Feature-gated drive_real compiles.
- **Completed:** 2026-06-13 (build phase)
- **Files modified:** crates/ferric-cli/src/{query.rs,main.rs}, crates/ferric-cli/Cargo.toml, Cargo.toml
- **Commit:** 7531057

## T-211 (sprint 2)
- **Description:** BenchSpec model (deny_unknown_fields; expectations file/dir/missing + content_regex; expected/any_of/forbidden tools; max_turns/timeout) + 7 embedded TOML specs L0–L6 ported to Ferric tool names (L0 forbids write/move/make_dir; L1 move_path; L2 make_dir). 5 parse tests.
- **Completed:** 2026-06-13 (build phase)
- **Files modified:** crates/ferric-bench/src/{spec.rs,lib.rs}, crates/ferric-bench/specs/l0..l6.toml
- **Commit:** 872b03d

## T-212 (sprint 2)
- **Description:** Bench runner: materialize workspace from setup_files, spawn-self `ferric query` (current_exe default; child always query → recursion impossible), std try_wait timeout-poll + kill, --keep-workspace (TempDir vs Kept), locate the single q-*.jsonl trace. Behavior exercised end-to-end by T-215's bench_mock test.
- **Completed:** 2026-06-13 (build phase)
- **Files modified:** crates/ferric-bench/src/{runner.rs,lib.rs}
- **Commit:** 55cc242

## T-213 (sprint 2)
- **Description:** verify.rs (parse_trace → TraceMetrics incl. tier/protocol/terminator/tokens/tools/summary, plan_steps null; verify_expectations file/dir/missing+regex; verify_tools required∧any_of∧¬forbidden; completed verdict; failure_admission phrases) + results.rs (ResultRow serde, append_row not-truncate, read_rows). 9 tests incl. verdict matrix + append-not-truncate.
- **Completed:** 2026-06-13 (build phase)
- **Files modified:** crates/ferric-bench/src/{verify.rs,results.rs,lib.rs}
- **Commit:** c165b40

## T-214 (sprint 2)
- **Description:** calibrate.rs: highest_completed_level → measured_level → ModelProfileRecord (tier_from_params vs tier_from_measured) → model_profiles.json (replace-same-key, keep-others). Exposed tier_for_params/tier_for_level from ferric-core. 4 tests incl. the 1B-completes-L4→Small override demonstration.
- **Completed:** 2026-06-13 (build phase)
- **Files modified:** crates/ferric-bench/src/{calibrate.rs,lib.rs}, crates/ferric-core/src/{scale.rs,lib.rs}
- **Commit:** dedfaf0

## T-215 (sprint 2)
- **Description:** `ferric bench` subcommand: --level/--protocol/--variant/--model-*/--params-b/--ctx/--prompts-dir/--results-dir/--keep-workspace/--ferric-bin/--mock; runs selected levels, appends results.jsonl, calibrates model_profiles.json; warns on debug-binary real sweeps. 3 model-free bench_mock integration tests (results written, per-level rows, keep-workspace).
- **Completed:** 2026-06-13 (build phase)
- **Files modified:** crates/ferric-cli/src/{bench_cmd.rs,main.rs}, crates/ferric-cli/Cargo.toml, crates/ferric-cli/tests/bench_mock.rs
- **Commit:** c729860

## T-216 (sprint 2)
- **Description:** Smoke refactored into run_smoke(protocol) + l0_smoke_native/l0_smoke_grammar #[ignore] variants (terminator ∈ {task_complete, final_text} both, C-010). ADR-015..019 recorded. Real-model execution (both smoke variants + L0–L4 calibration sweep ×2 protocols) runs in the Test phase per ADR-009.
- **Completed:** 2026-06-13 (build phase; real-model runs in Test phase)
- **Files modified:** crates/ferric-cli/tests/l0_smoke.rs, decisions.md
- **Commit:** (see git log for T-216)

## T-001 (sprint 7)
- **Description:** Reinstated the `Constraint` enum (`JsonSchema|Regex|Lark`) and `constraint: Option<Constraint>` on `CompletionRequest`; restored `validate()` to enforce ADR-010 (constraint XOR tools); added `supports_constraint` to `Capabilities` (honest, set false on every existing backend pending its own wiring); re-exported `Constraint`; updated all `Capabilities{}`/`CompletionRequest{}` literals. Also restored the CI gate to green from inherited s6 breakage: fixed the corrupted `.gitignore` (16 GB `models/` + logs were not actually ignored), stripped trailing whitespace + gated feature-only imports in `toolbench_cmd.rs`, removed dead `ToolCall`/`json` imports, and deleted the unreachable no-backend `create_provider` stub.
- **Completed:** 2026-06-22 (build phase)
- **Files modified:** .gitignore, crates/ferric-provider/src/{types.rs,lib.rs,mock.rs,mistralrs.rs,openai.rs,python.rs}, crates/ferric-provider/tests/mock_loop_skeleton.rs, crates/ferric-loop/src/{run.rs,protocol.rs}, crates/ferric-loop/tests/backoff_tests.rs, crates/ferric-cli/src/{backend.rs,query.rs,toolbench_cmd.rs}
- **Commit:** `e4a5684`

## T-002 (sprint 7)
- **Description:** `OpenAiProvider` now carries the harness constraint to the server. Extracted a pure `build_body(&CompletionRequest) -> Value`: a `Constraint::JsonSchema` becomes `response_format:{type:json_schema, json_schema:{name,schema,strict:true}}` (server-enforced, ADR-001 valve) with tools omitted; `Lark` maps to llama.cpp's `grammar` field; tools-without-constraint keeps native `tools`/`tool_choice`. `capabilities()` now honestly reports `supports_constraint:true`. Three model-free unit tests on the body shape + capability flags.
- **Completed:** 2026-06-22 (build phase)
- **Files modified:** crates/ferric-provider/src/openai.rs
- **Commit:** `c340ce8`

## T-003 (sprint 7)
- **Description:** Added the unified action grammar to ferric-loop: `action_schema(tools)` builds an `anyOf` of one const-discriminated `{tool,args}` branch per tool plus a `task_complete` branch (each `additionalProperties:false`), and `parse_json_action(turn,text)` parses the constrained `{"tool","args"}` completion into a `ToolCall` (id `g-<turn>-0`) with typed errors for non-object / missing tool / missing args. The XML `parse_action` is retained for the `TextXml` fallback. Exported all four from the crate root for the toolbench/loop. Six model-free unit tests.
- **Completed:** 2026-06-22 (build phase)
- **Files modified:** crates/ferric-loop/src/{grammar.rs,lib.rs}
- **Commit:** `fc0d8b2`

## T-004 (sprint 7)
- **Description:** Replaced the protocol dichotomy with an honest trichotomy `ActionProtocol { NativeTools, ConstrainedJson, TextXml }` (serde alias `unified_grammar` kept for old traces/bench rows). `select_protocol` now reads `Capabilities`: constraint→ConstrainedJson, native→NativeTools, neither→TextXml (override always wins). The loop (run.rs) builds the request per protocol — ConstrainedJson carries `Constraint::JsonSchema(action_schema(tools))` with empty tools and emits a TRUTHFUL `ConstraintApplied`, TextXml carries neither and emits none, NativeTools carries tools — and parses per protocol (tool_calls / parse_json_action / parse_action). Made mistral.rs `capabilities()` honest (neither native nor constraint → TextXml; was the s6 0.0% lie). CLI `--protocol {native,grammar,xml}`; query seeds caps from the chosen backend (OpenAI→constrained, mistral→xml). Added a `protocol-constrained-json` prompt atom and taught ferric-prompt all three (C-002). Cascaded the rename through trace/bench. Tests: protocol.rs trichotomy (4), new constrained_loop.rs (2, incl. real constraint on the request + ConstraintApplied), grammar_loop.rs→TextXml (asserts NO ConstraintApplied), truncation_tests→ConstrainedJson.
- **Completed:** 2026-06-22 (build phase)
- **Files modified:** crates/ferric-core/src/scale.rs, crates/ferric-loop/src/{run.rs,protocol.rs,grammar.rs}, crates/ferric-provider/src/mistralrs.rs, crates/ferric-cli/src/query.rs, crates/ferric-prompt/src/lib.rs, prompts/protocol-constrained-json.md, crates/ferric-trace/src/lib.rs, crates/ferric-bench/src/{runner.rs,results.rs}, crates/ferric-cli/tests/bench_mock.rs, crates/ferric-loop/tests/{common/mod.rs,grammar_loop.rs,truncation_tests.rs,constrained_loop.rs}
- **Commit:** `87ae78d`

## T-005 + T-006 (sprint 7)
- **Description:** Deleted the PyO3/PyTorch backend end-to-end (ADR-021): the `STATUS_HEAP_CORRUPTION` path that embedded CPython+PyTorch in the agent process, violating ADR-013 and the no-translational-layers rule. Landed as ONE commit because the two planned tasks are atomically coupled — removing `ferric-provider`'s `backend-python` feature breaks `ferric-cli`'s feature forwarding, so the tree can't compile with one without the other. T-005: deleted `crates/ferric-provider/src/python.rs` + the `python/` dir (incl. `inference.py`), dropped the `backend-python` feature and the `pyo3` dep from Cargo.toml, removed the `python` module/exports from lib.rs. T-006: removed `BackendArg::Python` + its match arm, all `feature = "backend-python"` cfgs (would trip `unexpected_cfgs` under -D warnings), the CLI feature declaration, and the python invocations in `test_both_models.ps1`/`run_benchmarks.ps1` (Gemma-4-e4b now reached via `--backend openai` behind Ollama). Verified `cargo tree --all-features` shows 0 pyo3.
- **Completed:** 2026-06-23 (build phase)
- **Files modified:** deleted crates/ferric-provider/src/python.rs + crates/ferric-provider/python/, crates/ferric-provider/{Cargo.toml,src/lib.rs}, crates/ferric-cli/{Cargo.toml,src/{backend.rs,query.rs,toolbench_cmd.rs}}, test_both_models.ps1, run_benchmarks.ps1, Cargo.lock
- **Commit:** `9bbe21b`

## T-007 (sprint 7)
- **Description:** Rebuilt the toolbench to measure the ACTIVE protocol's real fire rate instead of a native-only check that always read empty (the s6 0.0% bug). `extract_action(protocol, completion)` parses with the SAME parser the agent loop uses — native `tool_calls`, `parse_json_action` for ConstrainedJson, `parse_action` for TextXml — and a pass is a name match via that path. Added `--protocol` (defaults to the backend's real `capabilities()` via `select_protocol`, so the bench measures what `ferric query` runs). `build_request` sends the action-schema constraint (empty tools) for ConstrainedJson, tools for native, neither for TextXml. `extract_action` is gated `any(feature,test)` so its four dispatch unit tests run in the DEFAULT CI test job while the network-driving `run_toolbench` stays feature-gated. Also fixed a latent cli.rs test (`query_without_backend_errors`) that asserted the literal flag `backend-mistralrs` but the create_provider path says "mistralrs backend" — both name `mistralrs`; surfaced by running `cargo test --features backend-openai`.
- **Completed:** 2026-06-23 (build phase)
- **Files modified:** crates/ferric-cli/src/toolbench_cmd.rs, crates/ferric-cli/tests/cli.rs
- **Commit:** `a0f7693`

## T-008 (sprint 7)
- **Description:** Recorded ADR-021 (PyO3/PyTorch backend removed; external engines reached only via the out-of-process HTTP valve; closes the ADR-013 gap) and ADR-022 (Constraint reinstated, ADR-010 re-enforced, honest capabilities, the NativeTools/ConstrainedJson/TextXml trichotomy; amends ADR-015/ADR-020, fulfils ADR-017) in decisions.md. Corrected the two lying docs: ferric-provider/lib.rs module doc (now describes the two real backends + the PyO3 removal, not "real backends land in s1"), and README Status (was "Sprint 0 — no inference backend yet"; now describes the dual-backend constrained-decoding state). The architecture record now matches the code.
- **Completed:** 2026-06-23 (build phase)
- **Files modified:** decisions.md, crates/ferric-provider/src/lib.rs, README.md
- **Commit:** `d6ad065`

## T-801 (sprint 8)
- **Description:** Replaced the toolbench's `extract_action -> Option<ToolCall>` with a diagnostic `classify(protocol, completion, target, schema) -> Outcome { Success, WrongTool(name), MalformedArgs, NoAction, ParseError }` — it says *why* a model missed, not just pass/fail. Uses the same per-protocol parser as the loop (native tool_calls / parse_json_action / parse_action), distinguishes empty (NoAction) from non-empty-unparseable (ParseError), wrong-tool, and right-tool-missing-required-arg (lightweight `schema.required` check, not full JSON-Schema). `run_toolbench` now classifies and counts `is_success()`. Seven `cfg(test)` unit tests (one per Outcome + is_success), running in default CI. `label()` deferred to T-802 (the report consumer).
- **Completed:** 2026-06-23 (build phase)
- **Files modified:** crates/ferric-cli/src/toolbench_cmd.rs
- **Commit:** `c82fe72`

## T-802 (sprint 8)
- **Description:** Turned the toolbench into a written diagnostic. Added `BenchSummary`/`ToolStat` (per-tool fires/success/failure-histogram), `verdict(rate)` bands (≥90% solid / ≥70% marginal / else unreliable), pure `render_report(&BenchSummary) -> String` (Markdown table: per-tool rate + verdict + failure taxonomy, plus an overall band) and `summary_rows(&BenchSummary) -> Vec<Value>` (one JSONL row per tool + an `__overall__` row). `run_toolbench` accumulates the histogram via `Outcome::label()` and prints the report; `--report <path>` writes `<path>` (md) + a sibling `.jsonl`. This is the "is this model good enough — dial it down and watch where it breaks" readout the user asked for. Four unit tests (verdict bands, report taxonomy+verdict, JSONL shape, labels) in default CI.
- **Completed:** 2026-06-23 (build phase)
- **Files modified:** crates/ferric-cli/src/toolbench_cmd.rs
- **Commit:** `71e7a46`

## T-803 + T-804 (sprint 8)
- **Description:** Built the `ferric server` launcher (ADR-023). Landed as ONE commit because the pure `Engine`/`command`/`health_url` (T-803) are dead code until the subcommand that calls them (T-804) exists. New `server.rs`: `Engine { LlamaServer (default), Ollama }` (closed enum — never execs arbitrary input, ADR-005), `command(&ServerConfig)` builds the argv/env (`llama-server -m … --mmproj … -c … --host 127.0.0.1 --port …`; `ollama serve` + `OLLAMA_HOST`), `health_url`, and a `ServerRunfile {engine,pid,port,base_url}` written to `.ferric/server.json`. The `ferric server` subcommand: `up` (spawn child, TCP-connect readiness poll ≤60s, write runfile, leave it running), `status` (reachability + base_url), `down` (kill PID portably — taskkill/kill — + remove runfile), `doctor` (engine-binary + model presence + reachability). All std-only (TCP readiness, no reqwest), so it's in the default build; host pinned to loopback. 8 unit tests (argv/env, mmproj, loopback, health URLs, runfile serde, absent-runfile) in default CI; real spawn is the E2E heartbeat. Smoke-verified: status/down with no server + `up --help`.
- **Completed:** 2026-06-23 (build phase)
- **Files modified:** crates/ferric-cli/src/server.rs (new), crates/ferric-cli/src/main.rs, crates/ferric-cli/Cargo.toml (serde dep)
- **Commit:** `44bb01e`

## T-805 (sprint 8)
- **Description:** `query`/`toolbench` auto-discover the running server. Changed `BackendOpts.api_base` from a defaulted `String` to `Option<String>` and resolve it in `create_provider`'s OpenAI arm via `resolve_base(explicit, runfile)` — precedence **explicit `--api-base` > `.ferric/server.json` base_url > built-in default**. Reads the runfile from the cwd (where `ferric server up` wrote it). Both commands go through `create_provider`, so the one change covers both. `resolve_base` is a pure helper (gated openai+test) with a precedence unit test in default CI.
- **Completed:** 2026-06-23 (build phase)
- **Files modified:** crates/ferric-cli/src/backend.rs
- **Commit:** `020d418`

## T-806 (sprint 8)
- **Description:** Documented the testbench. Added a "First run — the testbench" section to the README (the `ferric server up` → `ferric toolbench --report` → read-the-verdict loop, noting auto-discovery + `server doctor`), bumped the Status marker to sprint 8, and wrote `docs/testbench.md` (full walkthrough: launch, the outcome taxonomy table, the verdict bands, and the dial-down workflow). Rewrote `run_benchmarks.ps1` and the Gemma path of `test_both_models.ps1` to wrap `ferric server up`/`down` around the toolbench/query instead of assuming a manually-started server.
- **Completed:** 2026-06-23 (build phase)
- **Files modified:** README.md, docs/testbench.md (new), run_benchmarks.ps1, test_both_models.ps1
- **Commit:** `4c180bd`

## T-901 (sprint 9)
- **Description:** Fleet sweep — added `--models <comma-list>` to `ferric toolbench`: extracted the per-model loop into `bench_model() -> BenchSummary` (reuses `classify`/`build_request`), the fleet path loops `create_provider` per model (overriding `BackendOpts.model`/`model_file` by backend), and `render_leaderboard()` prints a `model | protocol | success | rate | verdict` table sorted best→worst (+ a combined `.jsonl` of every model's `summary_rows`). Added a `model` field to `BenchSummary` (surfaced in the report header + JSONL rows). Single-`--model` behaviour unchanged. Unit test `leaderboard_sorts_best_first` asserts best→worst ordering + all three verdict bands.
- **Completed:** 2026-06-23 (build phase)
- **Files modified:** crates/ferric-cli/src/toolbench_cmd.rs
- **Commit:** `515bc11`

## T-902 (sprint 9)
- **Description:** Native-`content` fallback (ADR-024). The OpenAI backend's `complete()` now, when `tool_calls` is empty and `content` is itself a tool-call object, recovers the call via `toolcall_from_content()` — handling both the ollama shape (`{name, arguments}`) and the harness shape (`{tool, args}`), with `arguments` as a JSON object or a JSON-encoded string. Requires both a name and an args object so ordinary prose (or a stray JSON object) is never misread as a call. This closes the ADR-024 native-on-ollama 0% (ollama returns the call as text with `tool_calls` null). Collapsed into an edition-2024 let-chain. Four unit tests cover both shapes, string-encoded args, and prose→None.
- **Completed:** 2026-06-23 (build phase)
- **Files modified:** crates/ferric-provider/src/openai.rs
- **Commit:** `c3482e6`

## T-903 (sprint 9)
- **Description:** Documented fleet calibration. Added a "Calibrate the whole fleet at once" section to `docs/testbench.md` (the `--models` sweep, an example leaderboard, and how to read it top-down to pick the smallest model still in the band you need — noting it does not touch `measured_level`). Added the fleet sweep one-liner to the README testbench section. Extended `run_benchmarks.ps1` with an `$OllamaFleet` param + a fleet-sweep step (`--models … --report toolbench_fleet.md`) targeting a running ollama, with a note on the GGUF/mistral fleet alternative.
- **Completed:** 2026-06-23 (build phase)
- **Files modified:** docs/testbench.md, README.md, run_benchmarks.ps1
- **Commit:** `f913d78`

## T-1001 (sprint 10)
- **Description:** Multimodal core data model + routing logic (ADR-023). New `ferric-core::media` module: `Modality{Image,Audio,Video}`, `MediaPart{mime, data(base64)}`, and the pure routing functions `classify_path(path)->FileKind` (Text/Media/Unknown by extension) + `decide_attachment(kind, declared, backend_supports_media)->Attachment` (AppendText/Media/Skip-with-reason — media attaches only when the modality is declared AND the backend carries media) + `parse_modalities`. Added an **additive** `media: Vec<MediaPart>` field to `Message` (`#[serde(default, skip_serializing_if="Vec::is_empty")]`) so media-free messages serialize byte-identically (asserted) — plus `Message::user_with_media`. Threaded `media: Vec::new()` through every `Message{}` struct-literal site (openai, mistralrs, query mock, toolbench, loop test helpers). 7 new unit tests; green across default + backend-openai + backend-mistralrs.
- **Completed:** 2026-06-23 (build phase)
- **Files modified:** crates/ferric-core/src/{media.rs (new),message.rs,lib.rs}, crates/ferric-provider/src/{openai.rs,mistralrs.rs}, crates/ferric-provider/tests/mock_loop_skeleton.rs, crates/ferric-loop/tests/common/mod.rs, crates/ferric-cli/src/{query.rs,toolbench_cmd.rs}
- **Commit:** `e60d6d5`

## T-1002 (sprint 10)
- **Description:** OpenAI multimodal content mapping + honest media capability (ADR-023/022). `map_message` now emits the OpenAI **content-parts array** when a `Message` has media (a `text` part plus a `media_part_json` per item — `image_url` with a `data:<mime>;base64,…` URL for image/video, `input_audio` with base64+format for audio), and stays a plain string otherwise (unchanged). Added `Capabilities.supports_media` — `true` for the OpenAI valve (forwards parts), `false` for mistral.rs/mock/test backends — threaded through all 8 `Capabilities{}` sites. 3 new unit tests (string vs parts array, supports_media). Green across default + backend-openai + backend-mistralrs.
- **Completed:** 2026-06-23 (build phase)
- **Files modified:** crates/ferric-provider/src/{types.rs,openai.rs,mistralrs.rs,mock.rs}, crates/ferric-loop/src/protocol.rs, crates/ferric-loop/tests/backoff_tests.rs, crates/ferric-cli/src/query.rs
- **Commit:** `4ab6944`

## T-1003 (sprint 10)
- **Description:** `ferric query --file/--modality` "any file" input wiring (ADR-023). Added repeatable `--file <path>` + `--modality <image,audio,video>` to `QueryArgs`. In `run_query`, each file is routed via the pure `ferric_core` logic (`classify_path` + `decide_attachment` against the declared modalities + `caps.supports_media`): text/code → read and folded into the prompt (works on any model); media → base64'd into a gated `MediaPart` (new dependency-free `base64_encode` in `ferric-core::media`, RFC-4648 tested); Skip → surfaced on stderr, non-fatal. Threaded media into the loop: added `RunArgs.media` (`run.rs`) and the initial user message is now `Message::user_with_media(prompt, args.media.clone())` (empty ⇒ identical to before); updated all 4 `RunArgs{}` sites + both `drive_mock`/`drive_real` signatures (incl. the no-backend stub). 2 CLI integration tests (text file grows the assembled-prompt char count; a media file with no multimodal backend is skipped with a surfaced reason) + the base64 vectors. Green across default + backend-openai + backend-mistralrs; clippy `--all-targets -D warnings` clean.
- **Completed:** 2026-06-24 (build phase)
- **Files modified:** crates/ferric-core/src/{media.rs,lib.rs}, crates/ferric-loop/src/run.rs, crates/ferric-loop/tests/{backoff_tests.rs,common/mod.rs}, crates/ferric-cli/src/query.rs, crates/ferric-cli/tests/cli.rs
- **Commit:** `d8b2a1d`

## T-1004 (sprint 10)
- **Description:** Multimodal docs + README timeline. New `docs/multimodal.md` (the `--file`/`--modality` walkthrough: file-routing table, the ADR-006/022 gating rules, and how to run media E2E via `llama-server --mmproj`). Added a `--file` "any file" note to the README's Using-Ferric section, bumped Status to sprint 10, and appended the **Sprint 10** entry to the development timeline (with a Sprint 11 "Next" pointer).
- **Completed:** 2026-06-24 (build phase)
- **Files modified:** docs/multimodal.md (new), README.md
- **Commit:** `50b6d84`

## T-1101 (sprint 11)
- **Description:** Pass the decoding `Constraint` to the mistral.rs engine (ADR-027). `MistralRsProvider::complete()` no longer strips it (the s3 ADR-020 workaround) — it now maps our `Constraint::{JsonSchema,Lark,Regex}` 1:1 to `mistralrs::Constraint::*` via a pure `to_mistralrs_constraint` and applies `builder.set_constraint(…)` when present (unchanged when absent). `capabilities().supports_constraint` stays **`false` provisionally** — the wiring is present but unadvertised until the bounded `grammar_probe` proves enforcement-without-hang, so the loop won't route to a possibly-hanging path; the existing 5-min engine timeout is the belt-and-braces. Unit test `constraint_maps_to_mistralrs_variants` (matches! per variant). Gated `backend-mistralrs`; default workspace unaffected.
- **Completed:** 2026-06-24 (build phase)
- **Files modified:** crates/ferric-provider/src/mistralrs.rs
- **Commit:** `8c3fc60`

## T-1102 (sprint 11)
- **Description:** Probe verdict + revert (ADR-027). Ran the bounded `grammar_probe` through T-1101's wired provider on Llama-3.2-1B: with the constraint actually applied, `complete()` hit the **5-minute engine timeout** on the trivial `{x:string}` schema (probe ran 314 s then panicked) — a definitive **HANG**. So the ADR-020 llguidance-on-GGUF hang is **not** fixed in mistralrs 0.8.15 (ADR-025's "returns" had measured the stripped path). **Reverted** T-1101's `set_constraint` wiring (restored the strip, documented inline) — keeping it would 5-minute-hang `toolbench --backend mistral --protocol grammar`. `supports_constraint` stays false; mistral.rs stays `TextXml`; the HTTP valve remains the sole constrained path. Post-revert: clippy `--all-targets -D warnings` clean, 11 lib tests pass, fmt clean. ADR-027 + README/timeline updated.
- **Completed:** 2026-06-24 (test/loop phase)
- **Files modified:** crates/ferric-provider/src/mistralrs.rs, decisions.md, README.md
- **Commit:** `6db983d`

## T-1201 (sprint 12)
- **Description:** `search_files` builtin tool — the workspace content-search primitive a small coding agent needs to locate code before reading/editing. New `SearchFiles` (mirrors `list_dir`): recurses from `ctx.workspace.resolve(path|".")`, reads files as UTF-8 (read errors → binaries skipped for free), returns `relpath:lineno:line` for lines containing the literal `query` substring — sorted/deterministic (ADR-008, entries sorted before descent), capped at `max_results` (default 50, ADR-018), relpath via `strip_prefix(workspace.root())` + `/`-normalized. Skips noise dirs (`.git`/`target`/`node_modules`/`.ferric`). `permission: Read`, `min_tier: Nano`; `target_paths` returns the search root so the registry boundary-checks it (escapes Denied, ADR-005). Dependency-free substring (no `regex` dep). Registered in `register_builtin_tools`. 6 integration tests (hit+sorted+relpath/lineno, miss→empty, cap, binary+noise-dir skip, boundary-refusal, determinism). clippy `--all-targets -D warnings` clean.
- **Completed:** 2026-06-24 (build phase)
- **Files modified:** crates/ferric-tools/src/builtin/search_files.rs (new), crates/ferric-tools/src/builtin/mod.rs, crates/ferric-tools/tests/builtin_file_tools.rs
- **Commit:** `2de7bb7`

## T-1202 (sprint 12)
- **Description:** Documented `search_files`. Added a **Builtin tools** line to the README (lists all six workspace-scoped tools incl. `search_files` with its args + "find-before-edit" use), bumped Status to sprint 12, and appended the Sprint 12 development-timeline entry (with a Sprint 13 "Next" pointer — MCP-stdio).
- **Completed:** 2026-06-24 (build phase)
- **Files modified:** README.md
- **Commit:** `d2bea8b`

## T-1301 (sprint 13)
- **Description:** `edit_file` builtin — surgical replace of the first occurrence of `old_string` with `new_string` (`{path, old_string, new_string}`), the targeted edit small models do far more reliably than a full-file `write_file` rewrite. Resolves through `Workspace` (`permission: Write`); reads, `replacen(.., 1)`, writes back. Errors (no write) when `old_string` is empty, absent, or the file is unreadable. Mirrors `write_file.rs`. First-occurrence (not require-unique) maximizes small-model fire rate. 4 integration tests (replace-first, absent→error+unchanged, empty→error, outside-workspace→Denied). Completes Ring 0 of the tool-rings north star ([[ferric-tool-rings]]). (Built alongside T-1302 during a classifier outage; shared `builtin/mod.rs` + test file → one commit.)
- **Completed:** 2026-06-24 (build phase)
- **Files modified:** crates/ferric-tools/src/builtin/edit_file.rs (new), crates/ferric-tools/src/builtin/mod.rs, crates/ferric-tools/tests/builtin_file_tools.rs
- **Commit:** 05e3b57

## T-1302 (sprint 13)
- **Description:** `delete_path` builtin — delete a file or directory (`{path, recursive?}`). Resolves through `Workspace` (`permission: Write`, so the guard denylist auto-denies `.ferric`/git/ssh exactly like `write_file`/`move_path`); removes a file or **empty** dir, and a **non-empty** dir only with `recursive: true` (else a clear error — a small model can't accidentally nuke a tree). Uses `symlink_metadata` so a symlink is removed as a link, never followed. Missing path → clear error. Mirrors `move_path.rs`. 6 integration tests (file, empty-dir, non-empty needs-recursive + with-recursive, missing→error, outside-workspace→Denied, `.ferric`→Denied).
- **Completed:** 2026-06-24 (build phase)
- **Files modified:** crates/ferric-tools/src/builtin/delete_path.rs (new), crates/ferric-tools/src/builtin/mod.rs, crates/ferric-tools/tests/builtin_file_tools.rs
- **Commit:** 05e3b57

## T-1303 (sprint 13)
- **Description:** Docs — reframed the README builtin-tools list around **Ring 0** (the always-on `read_file/list_dir/write_file/make_dir/edit_file/delete_path` core) with `search_files` beyond it, and a one-line statement of the rings model (vocabularies widen with proven reliability; active rings = the grammar). Bumped Status to sprint 13; appended the Sprint 13 timeline entry with a Sprint 14 "Next" pointer (formalize the rings + fix the alphabetical `max_tools` cap).
- **Completed:** 2026-06-24 (build phase)
- **Files modified:** README.md
- **Commit:** 05e3b57

## T-1401 (sprint 14)
- **Description:** Formalized the tool **rings**. Replaced `ToolSpec.min_tier: Tier` with **`ring: u8`** (0 = core); the 8 builtins are `ring: 0` except `search_files` + `move_path` → `ring: 1`. Added `ferric_core::ring_for_tier(Tier) -> u8` (Nano→0, Small→1, Medium→2, Large/Xl/Ultra→3) — the capability→ring ceiling (honours `measured_level`, so a model is promoted by demonstrated reliability). Rewrote `tools_for_policy` to keep `ring ≤ ring_for_tier(tier)`, **trim from the outer ring first** when over `max_tools` (priority by `(ring, name)`, then name-sorted, ADR-008) — replacing the old alphabetical `.take` that could silently drop an essential core tool (e.g. `write_file` once 8 builtins exceeded the Nano cap of 6). `RunPolicy` unchanged ⇒ the tier-table snapshot stays put. Tests: `ring_ceiling_per_tier`; `tools_for_policy_trims_outer_ring_first` (core survives a cap, outer dropped, Nano sees only Ring 0); `rings_gate_builtins_by_tier` (Nano → exactly the 6 core, Small → all 8). Green across the workspace; clippy `--all-targets -D warnings` clean.
- **Completed:** 2026-06-24 (build phase)
- **Files modified:** crates/ferric-tools/src/{spec.rs,registry.rs,builtin/*.rs (8)}, crates/ferric-core/src/{scale.rs,lib.rs}, crates/ferric-tools/tests/builtin_file_tools.rs
- **Commit:** `6efca95`

## T-1402 (sprint 14)
- **Description:** ADR + docs for the ring architecture. **ADR-028** records the rings model (the `ring` field, `ring_for_tier` capability ceiling honouring `measured_level`, trim-from-outer `tools_for_policy` superseding the alphabetical cap, Ring 0/1 assignments, rings 2–3 reserved, and the `--max-ring`/measured-promotion follow-ons). README Status bumped to sprint 14 + the Sprint 14 timeline entry appended (with a Sprint 15 "Next" pointer). The builtin-tools section already frames Ring 0 (from sprint 13).
- **Completed:** 2026-06-24 (build phase)
- **Files modified:** decisions.md, README.md
- **Commit:** `f96f01e`

## T-1501 (sprint 15)
- **Description:** `RunPolicy.max_ring: Option<u8>` (`#[serde(default, skip_serializing_if)]`, `None` = the `ring_for_tier(tier)` ceiling; `policy_for` leaves it `None`). `tools_for_policy`'s ceiling is now `ring_for_tier(tier).min(max_ring.unwrap_or(u8::MAX))` — restrict-only (an override above the tier ceiling is a no-op; the trim-from-outer logic is unchanged, no signature change). The two `RunPolicy` test helpers call `policy_for` (not literals) so they inherit `None` — no change. Unit test `tools_for_policy_max_ring_override_caps`: `None`/`Some(1)` → all 8, `Some(0)` → the 6 core, `Some(5)` → no-op. (Built with T-1502 during a classifier outage → one commit.)
- **Completed:** 2026-06-24 (build phase)
- **Files modified:** crates/ferric-core/src/scale.rs, crates/ferric-tools/src/registry.rs
- **Commit:** 4a15eb0

## T-1502 (sprint 15)
- **Description:** `--max-ring` CLI flag (the user's "control exactly what rings"). `ferric query --max-ring <u8>` sets `policy.max_ring` after `policy_for`; `ferric toolbench --max-ring` benches rings `0..=N`. CLI integration test `max_ring_caps_the_offered_tools` runs `query --mock --params-b 8 --max-ring 0` and asserts the trace's `PromptAssembled.offered_tools` is the Ring-0 core (no `search_files`/`move_path`; `write_file` present) — proving the cap flows CLI → policy → `tools_for_policy` → grammar. ADR-028 amended (override shipped, restrict-only); README `--max-ring` note + Status sprint 15 + Sprint 15 timeline entry.
- **Completed:** 2026-06-24 (build phase)
- **Files modified:** crates/ferric-cli/src/{query.rs,toolbench_cmd.rs}, crates/ferric-cli/tests/cli.rs, decisions.md, README.md
- **Commit:** 4a15eb0

## T-1601 (sprint 16)
- **Description:** `ferric toolbench --calibrate-rings` — sweeps a model ring-by-ring and reports the highest ring it reliably drives (the recommended `--max-ring`). New `--calibrate-rings` flag; a calibrate branch in `run_toolbench` that, per `--models` entry (or the single configured model), loops `ring_cap=0,1,…` setting `policy.max_ring=Some(ring_cap)`, re-derives `tools_for_policy`+`action_schema`, runs the existing `bench_model`, records `(ring, tools, rate, verdict)`, and **stops when a ring adds no new tools** (auto-detects the max ring). Prints a per-model `ring|tools|rate|verdict` table + the recommendation; `--report` writes per-ring JSONL. Pure `recommend_max_ring(&[bool])->Option<u8>` = highest unbroken-`solid`-prefix ring (`None` if ring 0 not solid). `--calibrate-rings` supersedes a single `--max-ring`. Unit test `recommend_max_ring_longest_solid_prefix` (6 cases incl. break-after + ring-0-fail). Reuses `bench_model`/`verdict`/`overall_rate`; gated behind the backend features.
- **Completed:** 2026-06-24 (build phase)
- **Files modified:** crates/ferric-cli/src/toolbench_cmd.rs
- **Commit:** 84006aa

## T-1602 (sprint 16)
- **Description:** Calibration docs. `docs/testbench.md` §5 "Calibrate the rings — how far can this model go?" (the `--calibrate-rings` workflow, an example table, and how to feed the recommended ring to `--max-ring`). README: a calibration step #5 in the testbench section, Status bumped to sprint 16, and the Sprint 16 timeline entry (+ a Sprint 17 "Next" pointer toward persisting the calibrated ring). `run_benchmarks.ps1` gained a ring-calibration step (`--calibrate-rings --report toolbench_calib.md`).
- **Completed:** 2026-06-24 (build phase)
- **Files modified:** docs/testbench.md, README.md, run_benchmarks.ps1
- **Commit:** 84006aa

## T-1701 (sprint 17)
- **Description:** Profile read-back primitive (ferric-bench). Added `calibrated_ring: Option<u8>` to `ModelProfileRecord` (additive `#[serde(default)]` — records written before the field deserialize with `None`; `calibrate()` sets `None`). New `read_profile(dir, model, protocol) -> Option<ModelProfileRecord>` (exact (model, protocol) match; missing file/record → `None` — a safe no-op for the consumer). New `write_calibrated_ring(dir, model, protocol, params_b, ring)` — loads-or-creates the record and sets ONLY `calibrated_ring`, preserving any `measured_level` the L0–L6 bench wrote (reuses the `write_profile` merge-by-key discipline). Re-exported from lib.rs. 4 unit tests: read round-trip + wrong-key/missing → None; ring-merge preserves measured_level; create-when-absent; old JSON without the field → ring None.
- **Completed:** 2026-06-25 (build phase)
- **Files modified:** crates/ferric-bench/src/{calibrate.rs,lib.rs}
- **Commit:** 57b4c51

## T-1702 (sprint 17)
- **Description:** Persist + apply the profile (CLI), closing the read-back loop (ADR-029). `toolbench --calibrate-rings` gained `--profile-dir` (default `benchmarks`) and now writes each model's recommended ring via `ferric_bench::write_calibrated_ring` (keyed by model + the swept protocol label). `query` gained `--profile-dir` (default `benchmarks`): the caps-driven protocol is resolved up-front (it keys the lookup), then `ferric_bench::read_profile(profile_dir, model, protocol)` — model name from `backend_opts.model`/`model_file` — seeds `ModelProfile.measured_level` (→ tier) and defaults `policy.max_ring` to the record's `calibrated_ring`. Operator `--max-ring` still wins; a missing file / un-keyed model / mock-without-`--model` → `None` → byte-identical to before. CLI `--mock` test `persisted_calibrated_ring_caps_the_offered_tools` (a written `calibrated_ring: 0` caps the trace's `offered_tools` to the core; an empty profile-dir leaves Ring 1 intact). ADR-029 + README timeline (Status→17) + docs/testbench.md §5 "Make the promotion durable".
- **Completed:** 2026-06-25 (build phase)
- **Files modified:** crates/ferric-cli/src/{toolbench_cmd.rs,query.rs}, crates/ferric-cli/tests/cli.rs, decisions.md, README.md, docs/testbench.md
- **Commit:** fb29def

## T-1801 (sprint 18)
- **Description:** `find_files` builtin (Ring 1, Read) — the name-search companion to `search_files`' content search. `{pattern, path?: ".", max_results?: 100}` recurses from `path` and returns workspace-relative paths of files whose **name** contains `pattern`, name-sorted (ADR-008), capped (ADR-018), skipping noise dirs (`.git/target/node_modules/.ferric`); empty pattern → error. Mirrors `search_files.rs` (sorted walk, noise-skip). Registered in mod.rs. 1 unit test (finds by name + `path` scoping + cap + noise-skip + empty-pattern error). (Built with T-1802 — shared mod.rs + test file.)
- **Completed:** 2026-06-25 (build phase)
- **Files modified:** crates/ferric-tools/src/builtin/{find_files.rs,mod.rs}, crates/ferric-tools/tests/builtin_file_tools.rs
- **Commit:** 5baf4b0

## T-1802 (sprint 18)
- **Description:** `copy_file` builtin (Ring 1, Write) — the organize complement to `move_path`. `{from, to}` resolves+guards both endpoints, `create_dir_all`s the destination parent, `std::fs::copy`; a directory source errors (file-only — recursive copy out of scope). Write permission, so the destination denylist (`.ferric`, `.git/config`, ssh keys) applies. Mirrors `move_path.rs`. Registered in mod.rs. 3 unit tests (copies + keeps original + creates parent; copy into `.ferric` denied; directory source errors). Bumped `rings_gate_builtins_by_tier` 8 → 10 (Small now gets the 4-tool Ring 1: search_files, move_path, find_files, copy_file; still exactly `max_tools`=10; Nano unaffected at the 6 core). All ferric-tools tests green; clippy clean.
- **Completed:** 2026-06-25 (build phase)
- **Files modified:** crates/ferric-tools/src/builtin/{copy_file.rs,mod.rs}, crates/ferric-tools/tests/builtin_file_tools.rs
- **Commit:** 5baf4b0

## T-1803 (sprint 18)
- **Description:** Docs + re-bench for the Ring-1 round-out. README: the builtin-tools paragraph now describes Ring 1 as the four-tool "find & organize" set (`search_files`/`find_files`/`move_path`/`copy_file`); Status → sprint 18; Sprint 18 timeline entry (+ Sprint 19 pointer). decisions.md: ADR-028 sprint-18 amendment (Ring 1 rounded out; Small's max_tools=10 fits Ring 0+1 exactly; re-bench solid). **Re-bench (ollama): both qwen2.5-coder:7b AND llama3.2:1b still calibrate to `--max-ring 1` at 100%** with Ring 1 now 10 tools total — widening the ring cost zero reliability, even at 1B. `cargo test --workspace` green; clippy + fmt clean.
- **Completed:** 2026-06-25 (build/test phase)
- **Files modified:** README.md, decisions.md
- **Commit:** 7f5e8b9

## T-1901 (sprint 19)
- **Description:** `multi_edit` builtin (Ring 2, Write) — seeds Ring 2 ("plan & apply structured changes"). `{path, edits:[{old_string,new_string},…]}` reads the file once, applies each edit **sequentially** to an in-memory working copy via `replacen(_,_,1)` (a later edit may touch text an earlier one inserted), and writes **once** only if all validated — **atomic**: empty `edits`, an empty `old_string`, or an absent `old_string` at its turn → error with the file left byte-identical. Mirrors `edit_file.rs`; default `target_paths` guards `path` (Write → denylist). `ring: 2`. Registered in mod.rs. 3 unit tests (ordered batch incl. editing earlier-inserted text; missing-old leaves file unchanged; empty edits/old error). Bumped `rings_gate_builtins_by_tier`: added a Medium (params 20 → ring ceiling 2) case → 11 tools incl. `multi_edit`; asserted multi_edit absent at Small (10) and Nano (6). All 34 ferric-tools tests green; clippy clean.
- **Completed:** 2026-06-25 (build phase)
- **Files modified:** crates/ferric-tools/src/builtin/{multi_edit.rs,mod.rs}, crates/ferric-tools/tests/builtin_file_tools.rs
- **Commit:** 041dafb

## T-1902 (sprint 19)
- **Description:** `toolbench --params-b` + docs + live Ring-2 bench. Added `--params-b <f32>` to `ToolbenchArgs` (default 8.0) replacing the hardcoded `params_b: 8.0` in the bench profile, so the bench tier (hence ring ceiling) is selectable — `--params-b 20` → Medium → ring ceiling 2, letting `--calibrate-rings` sweep rings 0–2. README: builtin list now names `multi_edit` as Ring 2; Status → sprint 19; Sprint 19 timeline. decisions.md: ADR-028 sprint-19 amendment. docs/testbench.md: `--params-b` note for reaching higher rings. **Live E2E (the headline): qwen2.5-coder:7b at `--params-b 20` calibrates `--max-ring 2` — rings 0/1/2 (6/10/11 tools) all 100% solid.** The 7B drives the new nested-array `multi_edit` at 100% — Ring 2 is reachable; the constrained-decoding thesis holds for structured edits. `cargo test --workspace` green; clippy + fmt clean.
- **Completed:** 2026-06-25 (build/test phase)
- **Files modified:** crates/ferric-cli/src/toolbench_cmd.rs, README.md, decisions.md, docs/testbench.md
- **Commit:** bb99e3b

## T-2001 (sprint 20)
- **Description:** openai backend in the L0–L6 bench runner — so the full multi-turn agentic ladder can run the *constrained* path on a real model (it was `--mock`/mistral-GGUF-only, and mistral constrained hangs, ADR-027). Added additive `openai: Option<OpenAiArgs>` to `Invocation` (`OpenAiArgs{api_base: Option<String>, model, params_b, ctx}`); the 2 sites (`bench_cmd.rs`, `Invocation::mock()`) add `openai: None`. Extracted a pure `query_args(prompt, inv, workspace) -> Vec<String>` from `run_spec` (precedence: openai → mistral → `--mock`) so the backend branching is unit-testable without spawning. `bench` gained `--backend {mistral|openai}` (+ `--api-base`, `--model`); `--backend openai` builds the openai variant, keying the calibration record by the model id; defaults keep mistral/mock byte-identical. 3 `query_args` unit tests (openai arm has `--backend openai`/`--model`/`--api-base`, no `--model-dir`; mistral arm unchanged; mock arm). Verified manually: `query --backend openai` drives the loop (created hello.txt in 2 turns). default + openai builds + clippy + fmt clean.
- **Completed:** 2026-06-26 (build phase)
- **Files modified:** crates/ferric-bench/src/{runner.rs,lib.rs}, crates/ferric-cli/src/bench_cmd.rs
- **Commit:** 16b6097

## T-2002 (sprint 20)
- **Description:** Ran the L0–L6 ladder on the constrained backend + fixed the verification bug it surfaced + docs. **Bug:** `task_complete` is a structured terminator (SessionEnd), not a dispatched ToolCall, so `parse_trace` never credited it → every spec's `expected_tools=["task_complete"]` falsely failed (`tools_ok: false` despite `terminator: task_complete`). Fixed in `verify.rs` (credit the `task_complete` terminator as a called tool). **Result:** `qwen2.5-coder:7b` (ollama, ConstrainedJson) **passes all L0–L6** → `measured_level 6`, promoting Small→Large (ADR-019 override on real data); persisted to `model_profiles.json`; `query --profile-dir` reads back `measured_level Some(6)` (ADR-029, real data). Docs: ADR-030; README Status 20 + Sprint 20 timeline; docs/testbench.md §6 (`ferric bench` full-loop); run_benchmarks.ps1 L0–L6 step. `cargo test --workspace` green; clippy + fmt clean.
- **Completed:** 2026-06-26 (build/test phase)
- **Files modified:** crates/ferric-bench/src/verify.rs, decisions.md, README.md, docs/testbench.md, run_benchmarks.ps1
- **Commit:** 2570297

## T-2101 (sprint 21)
- **Description:** `bench --models` fleet sweep over the full L0–L6 agentic ladder. Extracted the per-level loop (run_spec → parse_trace → verify → append_row → print PASS/FAIL) into a shared `run_levels(selected, inv, protocol, model_name, args) -> (Vec<ResultRow>, bool)`; the single-model path now calls it (byte-identical — `bench_mock`/`l0_smoke` green). Hoisted `ferric_bin`. New `--models <a,b,c>` (openai backend): per model id → openai `Invocation`, `run_levels`, `calibrate` + `write_profile` (one record per model), then a `model | measured_level | tier` leaderboard sorted by level desc (ADR-008). The fleet returns SUCCESS (a low measured_level is a valid measurement, not a failure); the single path keeps its FAILURE-on-any-level-fail contract. Imports `BenchSpec`/`ModelProfileRecord`. default build + openai clippy + fmt clean.
- **Completed:** 2026-06-26 (build phase)
- **Files modified:** crates/ferric-cli/src/bench_cmd.rs
- **Commit:** 8e62121

## T-2102 (sprint 21)
- **Description:** Ran the fleet L0–L6 sweep + docs. **Agentic capability map (ollama, ConstrainedJson):** qwen2.5-coder:7b → measured_level 6 (Large, all pass); llama3.1:8b → 5 (Medium; passes L0–L3,L5, fails L4,L6); llama3.2:1b → none (fails even L0). **Findings:** (1) single-tool-call reliability ≠ agentic capability — the 1B fires single tool calls at 100% (toolbench) but can't *complete* a multi-turn task; (2) the code-tuned 7B beats the larger general 8B; (3) the ladder discriminates (6/5/none) so L7+ isn't urgent. Per-model `measured_level` persisted to `model_profiles.json`. Docs: ADR-030 sprint-21 amendment (fleet map + findings); README Status 21 + Sprint 21 timeline; docs/testbench.md §6 `--models` fleet note; run_benchmarks.ps1 fleet bench step. `cargo test --workspace` green; clippy + fmt clean.
- **Completed:** 2026-06-26 (build/test phase)
- **Files modified:** decisions.md, README.md, docs/testbench.md, run_benchmarks.ps1
- **Commit:** c3968b1

## T-2201 (sprint 22)
- **Description:** Sharpened the repetition-guard `Verdict::Warn` nudge (`run.rs`) from the soft/conditional "You are repeating the same tool calls. Take a different action, or call task_complete if the task is done." to a **direct imperative naming the repeated tool(s)**: "You already called `<tool>` and have the result — do not call it again. If the task is finished, call task_complete now with a one-sentence summary." (built from the turn's `actions` names). Targets the diagnosed 1B failure mode (repeat-not-terminate — s21 finding). Two-strike guard *behavior* unchanged (wording only). Updated `repetition_tests.rs` to assert the nudge contains `task_complete` (stable directive) instead of `repeating`; `["warned","stopped"]` + `StopReason::RepetitionGuard` unchanged. ferric-loop tests + clippy + fmt clean.
- **Completed:** 2026-06-26 (build phase)
- **Files modified:** crates/ferric-loop/src/run.rs, crates/ferric-loop/tests/repetition_tests.rs
- **Commit:** 9d6bd37

## T-2202 (sprint 22)
- **Description:** Re-benched llama3.2:1b L0–L6 with the sharper nudge + ADR + docs. **Result: no change — still `measured_level: none`, identical failure modes** (L0 `repetition_guard` ['list_dir','list_dir']; L1 repeats read_file/make_dir; L2 `max_turns` after 15 distinct make_dir = "semantic flailing" the guard misses). So the hypothesis (wording is the bottleneck) is **disproven**: the 1B's multi-turn failure is a genuine capability limit (planning/state-tracking/completion-recognition), not nudge text. Decisions (ADR-031): ship the nudge anyway (better wording, helps mid-tier models, can't regress capable ones); the 1B's role is settled as a reliable constrained tool-caller, not an agent (the tier machinery already encodes this); record a no-progress/max-same-tool guard for semantic flailing as future hardening. README Status 22 + Sprint 22 timeline.
- **Completed:** 2026-06-26 (test phase)
- **Files modified:** decisions.md, README.md
- **Commit:** 260cdd1

## T-2301 (sprint 23)
- **Description:** Made llama.cpp (`llama-server`) the first-class engine — ADR-032 + a llama-server-first guide — and **validated it live for the first time**. The launcher already defaulted to llama-server and was already contract-tested (`server::command()`: `llama_server_argv`/`llama_server_mmproj`/`ollama_argv_and_env`), so **no launcher code change was needed**; the work is the validation + docs. Live (sprint 23 test phase): fetched the prebuilt `b9821` CPU/x64 release, pointed `llama-server -m` at an **ollama GGUF blob** (no re-download — ollama blobs are raw GGUF), and drove it with `ferric --backend openai --api-base :8080/v1 --protocol grammar` → the constrained loop created a file and a Ring-0 toolbench scored **36/36 = 100% solid, identical to ollama**. So the OpenAI constrained valve is engine-agnostic and works on full llama.cpp. Docs: `decisions.md` ADR-032; new `docs/llama-cpp.md` (install, ollama-blob trick, `-c` wide context, `--mmproj` multimodal, Jetson/Pi edge notes); README leads with `--engine llama-server` + Status 23 + Sprint 23 timeline. `cargo test --workspace` green; clippy + fmt clean.
- **Completed:** 2026-06-26 (build/test phase)
- **Files modified:** decisions.md, docs/llama-cpp.md, README.md
- **Commit:** 9e33741

## T-2401 (sprint 24)
- **Description:** **Validated the multimodal pipeline end-to-end** (the marquee goal deferred since sprint 10) + ADR-033 + docs. No Ferric code change — the `image_url`/base64 content-parts mapping (`openai.rs:media_part_json`) + `--file`/`--modality` routing were already built + unit-tested (s10). Live: fetched SmolVLM-500M-Instruct GGUF (436MB) + its mmproj (108MB) from ggml-org, served via prebuilt llama.cpp `b9821` (`llama-server -m … --mmproj …`); a generated 96×96 red square went through `ferric query --file --modality image` → server log `process_mtmd: encoding mtmd batch n_chunks=1` (image reached the vision encoder), and a direct query in Ferric's exact `image_url` format returned **"Red."** — the model sees what Ferric sends. Finding: under the constrained JSON grammar a sub-1B VLM degrades free-form captioning (echoed a system-prompt line into task_complete); the image still reaches the model — use a bigger VLM or an unconstrained describe. decisions.md ADR-033; docs/llama-cpp.md §5 validated multimodal walkthrough; README Status 24 + Sprint 24 timeline. `cargo test --workspace` green; clippy + fmt clean.
- **Completed:** 2026-06-26 (build/test phase)
- **Files modified:** decisions.md, docs/llama-cpp.md, README.md
- **Commit:** f08412e

## T-2501/T-2502/T-2503 (sprint 25)
- **Description:** **Validated Gemma 4 E4B as Ferric's reference ~4B multimodal model** (research pivot from a `--chat` workaround — a capable model is the right answer to the ~4B agentic floor). No Ferric code change (validation, like s23/s24). Downloaded the official ungated `google/gemma-4-E4B-it-qat-q4_0-gguf` (model 5.15GB QAT-q4 + mmproj 0.99GB); the existing prebuilt `b9821` llama-server loaded the Gemma 4 arch + mmproj with **no update needed**. **Results:** (1) **Multimodal inside the constrained agentic loop** — `ferric query --file red.png --modality image --protocol grammar` → `task_complete("The image is a solid red rectangle.")`, closing the ADR-033 caveat with no harness change (where SmolVLM-500M garbled). (2) **Agentic L0–L6 bench → measured_level 5** (PASS L1/L3/L4/L5) — **matches the 8B (5), below the 7B (6), vastly above the 1B (none)**, confirming ~4B as the usable agentic floor (L0/L2/L6 fails mostly CPU-speed timeouts — L0 hit the 60s cap). (3) **Ring-0 toolbench 100% solid**. ADR-035 (Gemma 4 E4B = recommended reference model; ~4B floor; capability closes ADR-033); docs/llama-cpp.md §5 Gemma 4 quickstart + GPU-speed note; README Status 25 + Sprint 25 timeline. `cargo test --workspace` green; clippy + fmt clean.
- **Completed:** 2026-06-27 (build/test phase)
- **Files modified:** decisions.md, docs/llama-cpp.md, README.md
- **Commit:** b6a44ab

## T-2601/T-2602 (sprint 26)
- **Description:** **Validated Gemma 4 E4B's audio modality end-to-end** (the other half of multimodal after vision in s24/25) + ADR-036 + docs. No Ferric code change — `media_part_json` already maps `audio/*` → an OpenAI `input_audio` content block (s10). Confirmed (web) that llama.cpp added Gemma 4 audio via a Conformer encoder (PR #21421) + llama-server accepts `input_audio`. Live (cached Gemma 4, no download): the prebuilt `b9821` llama-server loaded the audio encoder (`init_audio`); a Windows-TTS 16kHz-mono WAV of "The quick brown fox jumps over the lazy dog." → `ferric query --file speech.wav --modality audio --protocol grammar "transcribe … then task_complete"` → **`task_complete("The quick brown fox jumps over the lazy dog.")`** — exact ASR, 1 turn, inside the constrained loop. So Ferric multimodal is now **vision + audio**, both live on the reference model via one llama.cpp binary. decisions.md ADR-036; docs/llama-cpp.md §5 audio example; docs note; README Status 26 + Sprint 26 timeline. `cargo test --workspace` green; clippy + fmt clean.
- **Completed:** 2026-06-27 (build/test phase)
- **Files modified:** decisions.md, docs/llama-cpp.md, README.md
- **Commit:** 3040626

## T-2701/T-2702/T-2703 (sprint 27)
- **Description:** **A no-progress guard closing ADR-031's second failure mode ("semantic flailing").** The repetition guard hashes the COMPLETE action signature (name + args), so it never fires on the same tool with *different* args every turn (`make_dir` ×15 → grinds to `max_turns`). Added `ProgressGuard` (`crates/ferric-loop/src/progress.rs`) mirroring `RepetitionGuard` but canonicalizing only the **sorted-unique tool NAMES** (arg-insensitive, order-independent via `BTreeSet`): Warn at `WARN_AT=4`, Stop at `STOP_AT=5` → `StopReason::NoProgress` (`Event::NoProgressGuard{action}`), wired right after the repetition guard in `run.rs`. Threshold ~6 turns — above realistic same-tool runs, under every tier's `max_turns` (Nano 15 … Large 40); the guards compose (repetition fires earlier on identical calls). 6 new tests incl. **the defining contrast** (`ProgressGuard`→Stop where `RepetitionGuard`→Proceed on the same input) + integration mirroring `repetition_tests`; the `max_turns` tests alternate tool names to isolate the budget. No bench change (`completed()` already treats non-terminator reasons as non-completions). ADR-037; README Status 27 + Sprint 27 timeline. Honest scope: bounds wasted compute + sharpens the diagnostic; does not lift a capability ceiling. `cargo test --workspace` green; clippy `-D warnings` + fmt clean.
- **Completed:** 2026-06-27 (build/test phase)
- **Files modified:** crates/ferric-loop/src/{progress.rs (new),lib.rs,outcome.rs,run.rs}, crates/ferric-trace/src/event.rs, crates/ferric-cli/src/trace_cmd.rs, crates/ferric-loop/tests/{progress_tests.rs (new),common/mod.rs,loop_core.rs}, decisions.md, README.md, agent-tasks/*
- **Commits:** 80ffd8a (code+tests), de3442c (ADR+docs)

## T-2801/T-2802/T-2803 (sprint 28)
- **Description:** **A repeated-failure guard completing the loop-hardening guard family.** The repetition guard (name+args) and no-progress guard (tool name) both key off the *actions* a model emits — neither catches a model emitting a *different* tool every turn that *all error* (bad paths, denials, malformed args): both reset, so it grinds to `max_turns`. Added the result-keyed `FailureGuard` (`crates/ferric-loop/src/failure.rs`): `observe_turn(dispatched, errored)` — a "failure turn" is ≥1 dispatched tool with **all** errored (any success resets; a zero-dispatch turn never trips); Warn at `WARN_AT=2`, Stop at `STOP_AT=3` → `StopReason::RepeatedFailure` (`Event::FailureGuard{action}`). Wired **after** the dispatch loop in `run.rs` (it keys off `is_error` results), gated on a non-terminating turn. The three guards now compose by threshold (repetition 2 < failure 3 < no-progress 5). 6 new tests incl. the integration that stops **different** failing tools every turn while repetition + no-progress stay silent (their gap). No bench change (`completed()` already excludes non-terminator reasons). ADR-038; README Status 28 + Sprint 28 timeline. Honest scope: bounds wasted compute + sharpens the `repeated_failure` vs `max_turns` diagnostic; no capability lift. `cargo test --workspace` green; clippy `-D warnings` + fmt clean.
- **Completed:** 2026-06-27 (build/test phase)
- **Files modified:** crates/ferric-loop/src/{failure.rs (new),lib.rs,outcome.rs,run.rs}, crates/ferric-trace/src/event.rs, crates/ferric-cli/src/trace_cmd.rs, crates/ferric-loop/tests/{failure_tests.rs (new),common/mod.rs}, decisions.md, README.md, agent-tasks/*
- **Commits:** 22a6bdc (code+tests), 2da60e6 (ADR+docs)

## T-2901/T-2902/T-2903 (sprint 29)
- **Description:** **`apply_patch` rounds out Ring 2** (the rings-memory "room to grow"; pivot from the now-complete guard family back to the tool rings). Added `crates/ferric-tools/src/builtin/apply_patch.rs` (`ring: 2`, `PermissionLevel::Write`): applies a context-located unified diff to one file, atomically. Args `{path, patch}`; the patch is `@@`-delimited hunks with ` `/`-`/`+ lines — **`@@` line numbers ignored, hunks matched by context**. Apply is line-based (`split('\n')` → locate the first contiguous `context+removed` run → splice `context+added` → `join("\n")`, round-trips the trailing newline) and atomic (write **once** only if all hunks locate; a failure leaves the file byte-identical, like `multi_edit`). **Distinct from `multi_edit`:** a hunk's context **disambiguates** which occurrence to edit (multi_edit's `replacen` hits only the first) + diff-format familiarity. Registered in `builtin/mod.rs`; `rings_gate_builtins_by_tier` Medium 11→**12** (Ring 0 + Ring 1 + multi_edit + apply_patch), Nano 6 / Small 10 unchanged. 5 tests incl. **the defining contrast** (a hunk whose context pins the 2nd of two identical lines edits that one — impossible with multi_edit); absent-context → error + byte-identical; empty/malformed → error; multi-hunk in order. Single-file scope (multi-file deferred). No registry/scale change (Medium max_tools=16 ≥ 12). ADR-039; README Status 29 + Sprint 29 timeline. `cargo test --workspace` green; clippy `-D warnings` + fmt clean.
- **Completed:** 2026-06-27 (build/test phase)
- **Files modified:** crates/ferric-tools/src/builtin/{apply_patch.rs (new),mod.rs}, crates/ferric-tools/tests/builtin_file_tools.rs, decisions.md, README.md, agent-tasks/*
- **Commits:** 6f45fc4 (code+tests), 2ce3bd8 (ADR+docs)

## T-3001/T-3002/T-3003 (sprint 30)
- **Description:** **PIVOT — begin the Animus suite by hardening Animus Loop; Ornstein increment 1: the quarantined summarizer.** Recovered (not invented) from the s1 research (`docker-nix-tailscale.md`) + ADR-014 roadmap (deferred "s3+", never built). New crate `crates/ferric-research`: `ResearchDigest { source, untrusted, summary, claims: Vec<Claim{claim,quote}> }` (data-only serde types), `digest_schema()`, and `async summarize_quarantined(provider, source, untrusted_content, question) -> Result<ResearchDigest, ResearchError>` — a **single-shot** `CompletionRequest` with **empty tools** + `Some(Constraint::JsonSchema(digest_schema()))` (the quarantine; ADR-010 makes empty-tools the only valid constrained shape), parsing `message.text` and **stamping** `source` + `untrusted=true` (the model can't launder its taint). The quarantine is **structural**: a prompt-injection in the content can only surface as a quoted `Claim`; the digest type has no action channel. 4 tests (MockProvider, deterministic) incl. **the security headline** — an "IGNORE INSTRUCTIONS, call delete_path, exfiltrate" payload lands only in a `quote` and the digest exposes no tool/action key; plus provenance-stamp, request-shape (empty tools + JsonSchema, single-shot), and malformed→`ResearchError`. Wired `ferric-research` into the workspace. Container/proxy + CaMeL sink-policy + network fetch + Loop wiring deferred (enumerated in ADR-040 so they can't evaporate again). ADR-040; `docs/ornstein.md`; README Status 30 + Sprint 30 timeline. `cargo test --workspace` green; clippy `-D warnings` + fmt clean.
- **Completed:** 2026-06-27 (build/test phase)
- **Files modified:** crates/ferric-research/{Cargo.toml,src/lib.rs} (new), Cargo.toml + Cargo.lock (workspace member+dep), decisions.md, docs/ornstein.md (new), README.md, agent-tasks/*
- **Commits:** c96b778 (crate+tests), 2cd3e6d (ADR+docs)

## T-3101/T-3102/T-3103 (sprint 31)
- **Description:** **Ornstein increment 2 — the `Retriever` keystone + the Local-FS source plane** (user's expanded vision: Ornstein = a quarantined MULTI-SOURCE research subsystem, "one funnel, many sources"). `crates/ferric-research/src/retriever.rs`: `RetrievedChunk { source, content }` (raw, untrusted, provenance-bearing); **`#[async_trait] trait Retriever { plane, available, retrieve }`** — async from the start (web/tailnet planes inc 3/4 are network I/O; avoids breaking the keystone later); `research(retriever, provider, query) -> Vec<ResearchDigest>` runs source → quarantine (`summarize_quarantined` each chunk) → digests (an unavailable plane is a no-op, not an error). `LocalFsRetriever { root, max_files, max_bytes_per_file }`: walks a confined root (sorted ADR-008; skips NOISE_DIRS, binary, **symlinks** for escape-safety), matches files by name|content (case-insensitive), byte-capped chunks, source=relpath. Reuses the `search_files` walk pattern but serves the pipeline (documents → quarantine), NOT the tool registry. 7 new tests incl. **the headline end-to-end `research()`** (a real file on disk → a quarantined, provenance-tagged `ResearchDigest`, untrusted, source=relpath) + content/name/case-insensitive match, noise+binary skip, max_files cap, availability. `ResearchError` gains `Retrieve`; `async-trait` + `tempfile` added to the crate. Build order (user): Local FS (this) → Tailnet/NAS FS → Web+container. ADR-041; `docs/ornstein.md` Sources section; README Status 31. `cargo test --workspace` green; clippy `-D warnings` + fmt clean.
- **Completed:** 2026-06-27 (build/test phase)
- **Files modified:** crates/ferric-research/{Cargo.toml,src/retriever.rs (new),src/lib.rs}, Cargo.lock, decisions.md, docs/ornstein.md, README.md, agent-tasks/*
- **Commits:** e127f94 (code+tests), f9360ba (ADR+docs)

## T-3201/T-3202/T-3203 (sprint 32)
- **Description:** **Ornstein increment 3 — the Tailnet/NAS-FS retriever (Tailscale SSH).** The second source plane behind the keystone: search a *remote* tailnet device's filesystem over SSH, feed matches to the same quarantine. `crates/ferric-research/src/retriever.rs`: `SshTransport { Tailscale, Plain{port} }` (`tailscale ssh` for Linux devices; plain `ssh -p` for Termux); **`shell_single_quote`** — the security core: `ssh` runs its command via the *remote* shell, so the caller-supplied query + root are POSIX single-quote-escaped or it's **remote command injection**; `ssh_search_argv`/`ssh_cat_argv` build injection-safe argv (`grep -rIl -- 'Q' 'ROOT' | head -n N`; `cat -- 'PATH'`); `parse_status_devices(stdout) -> [{name,ip,online}]`. `TailnetFsRetriever` impl `Retriever`: `plane()="tailnet"`, `available()` = host online in `tailscale status`, `retrieve()` spawns search→cat → `host:relpath` chunks. 6 new tests (17 in the crate): injection-escaping (`;rm`/`$()`/backticks), both transport argv forms, path escaping, the real `tailscale status` sample parse. **Pure core / live spawn split (server.rs precedent): the deterministic core ships + is tested; the live SSH E2E is DEFERRED (user's call) — live probe found no reachable sshd (pixel-10-pro-xl reachable but no sshd on :22/:8022; switchblade offline).** `RetrieveError` gains `Exec`; re-exports updated. ADR-042; `docs/ornstein.md` tailnet section; README Status 32. `cargo test --workspace` green; clippy `-D warnings` + fmt clean.
- **Completed:** 2026-06-28 (build/test phase)
- **Files modified:** crates/ferric-research/src/{retriever.rs,lib.rs}, decisions.md, docs/ornstein.md, README.md, agent-tasks/*
- **Commits:** c14ba57 (code+tests), 640b761 (ADR+docs)

## T-3301/T-3302 (sprint 33)
- **Description:** **Ornstein — the research orchestrator (`research_all` across planes).** The "one funnel, many sources" payoff: run a query across every available plane at once. `crates/ferric-research/src/retriever.rs`: `research_all(retrievers: &[&dyn Retriever], provider, query) -> Result<MultiResearch, ResearchError>` — per retriever in order: probe `available()`; if available, `retrieve` then quarantine each chunk whose `source` is new (**cross-plane `BTreeSet` dedup BEFORE the model call** — a source from two planes costs one inference); push a `PlaneResult{plane, available, digests}`. Returns `MultiResearch{ digests (plane-ordered, deduped), planes (per-plane report) }`. Unavailable planes = recorded no-ops, never errors. `research()` (single-plane) untouched; new items re-exported. 4 new tests (21 in the crate): multi-plane aggregate, **cross-plane dedup proven via a one-completion MockProvider script** (dedup precedes inference), unavailable-plane skip, all-unavailable empty. Composes the local + tailnet planes with zero pipeline change; Web plane (inc 4) still gated on a containerizer (docker re-probed absent on Windows + WSL). ADR-043; `docs/ornstein.md` orchestrator section; README Status 33. `cargo test --workspace` green; clippy `-D warnings` + fmt clean.
- **Completed:** 2026-06-28 (build/test phase)
- **Files modified:** crates/ferric-research/src/{retriever.rs,lib.rs}, decisions.md, docs/ornstein.md, README.md, agent-tasks/*
- **Commits:** 9e0d235 (code+tests), 12fe6d0 (ADR+docs)

## T-3401/T-3402 (sprint 34)
- **Description:** **Ornstein — the CaMeL-lite sink-policy primitive.** Co-designed with the user: flow control on top of the quarantine. A digest's text is untrusted, but nothing previously stopped the model from echoing it into a tool argument reaching a dangerous sink. New `crates/ferric-research/src/sink.rs`: `TaintSet` (CaMeL-lite substring taint tracking — `taint_digest(&ResearchDigest)` marks the summary + each claim quote; `is_tainted`/`args_tainted` — the latter recursively walks a tool-call args JSON) + `SinkPolicy::decide(permission: PermissionLevel, tainted: bool) -> SinkDecision`, keyed off the existing `ferric_guard::PermissionLevel`: untainted → `Allow`; `Read`+tainted → `Allow` (reading isn't a dangerous sink); `Write`/`Execute`+tainted → the configured `SinkAction` (**all 3 modes ship — `Deny`/`RequireApproval`/`Warn`, caller picks**, per the user's explicit choice). 8 new tests (29 in the crate) incl. **the end-to-end gate shape**: a tainted digest's injected quote, echoed into `write_file` args, is flagged tainted and `Deny`d under the autonomous default — the structural proof of the gate the eventual wiring will enforce. **Pure primitive only — NOT wired into dispatch.** Added `ferric-guard` dep (no cycle); the deferred enforcement point is `registry.execute` beside the existing `check(permission, path)`, once the research→loop wiring populates the `TaintSet`. ADR-044; `docs/ornstein.md` CaMeL section + sample; README Status 34. `cargo test --workspace` green; clippy `-D warnings` + fmt clean.
- **Completed:** 2026-06-28 (build/test phase)
- **Files modified:** crates/ferric-research/{Cargo.toml,src/sink.rs (new),src/lib.rs}, decisions.md, docs/ornstein.md, README.md, agent-tasks/*
- **Commits:** dd194ec (code+tests), 2ccf9bf (ADR+docs)

## T-3501/T-3502/T-3503/T-3504/T-3505 (sprint 35)
- **Description:** **Expert review + refactor — the first full-project audit.** Direct, file:line-cited audit of security/efficiency/product-completeness (the 3 background review agents were stopped by the user before completing and were not relaunched), cross-referenced against a corrected external review (GLM-5-turbo). Full findings in `sprints/s35/sprint-research/research-report.md`. Four immediately-effective fixes shipped: **(1) Read-side sensitive-file guard** (`crates/ferric-guard`) — `PermissionLevel::Read` previously unconditionally `Allow`d; added `DENIED_READ_SEGMENTS` (credential stores minus `.git`) + `DENIED_READ_FILES` (write-file list + `.env`), closing a real secret-into-plaintext-trace gap while keeping `.git` metadata reads legitimate. **(2) `ferric server` edge-tuning flags** (`crates/ferric-cli/src/server.rs`) — `--threads`/`--gpu-layers`/`--batch-size` → `-t`/`-ngl`/`-b` for llama-server (Ollama accepts-but-ignores); backward compatible (byte-identical argv when omitted). **(3) `mistralrs` rev-pinned** (`Cargo.toml`) — was floating on `branch = "master"`; resolved current master HEAD via `git ls-remote` and pinned, matching the `oovra` reproducibility policy; verified `--features backend-mistralrs` still builds. **(4) `reqwest` → `rustls-tls`** (`Cargo.toml`) — `cargo tree -e features` confirmed `default-tls` (native OpenSSL) was active; switched to pure-Rust `rustls-tls` (Ferric only calls `http://127.0.0.1` per ADR-005, so TLS itself was dormant — zero exercised behavior change, pure dependency-weight/cross-compile win). 7 new tests across `ferric-guard`/`ferric-cli` incl. regressions proving the guard doesn't overreach (`.git/config` reads, `.env` writes stay allowed). Panic-safety sub-audit (grepped `unwrap`/`expect`/`panic!` across model-output/backend-response/file-content paths) came back **clean**. Explicitly deferred with reasons (ADR-045): CaMeL sink-policy wiring (no live taint source yet — dead plumbing if wired now), `ferric mcp` + the new chat mode (already decided, own dedicated sprint), shell/git tools, streaming, session resume, trace rotation. ADR-045; README Status 35 + Sprint 35 timeline. `cargo test --workspace` green (default + `backend-mistralrs` features); clippy `-D warnings` clean (both feature sets); fmt clean.
- **Completed:** 2026-06-29 (build/test phase)
- **Files modified:** crates/ferric-guard/src/{denylist.rs,checker.rs}, crates/ferric-cli/src/server.rs, Cargo.toml, Cargo.lock, decisions.md, README.md, agent-tasks/*
- **Commits:** 857e9ad (T-3501), 435ca58 (T-3502), 02b98c8 (T-3503), accc8b2 (T-3504), 96c1e32 (T-3505 ADR+docs)
## T-3601/T-3602/T-3603/T-3604/T-3605/T-3606/T-3607 (sprint 36)
- **Description:** **`ferric mcp` — the ADR-005 security call + the MCP-stdio server it unblocks.** User-prioritized from the GLM-review "critical gaps" list; the companion mistral.rs in-process-hang item was explicitly dropped (reprobed twice, ADR-020/027 — the HTTP valve remains the only backend that matters). **The security call:** `ferric mcp` exposes **exactly one** MCP tool, `ferric_query` (`{prompt, files?}`) — never Ferric's individual builtins, and never workspace/backend/model as per-call parameters (those are launch-time-fixed `McpArgs` CLI flags). The tool schema has no `workspace`/`backend`/`model` field, so a client can't redirect containment or swap the model per call — the guarantee is **structural** (unrepresentable in the wire protocol), proven by a dedicated schema-shape test. Every `tools/call` runs the same constrained agent loop `ferric query` drives, inheriting the guard/permission checks, tool rings, and per-call JSONL tracing. **Hand-rolled JSON-RPC 2.0** (no `rmcp` dependency — the surface is one tool, no resources/prompts/notifications). Tasks: **T-3601** split provider construction from loop execution (`run_with_provider`, reusable given only a `&dyn Provider`); **T-3602** extracted the launch-time-fixed run-config builder (`RunConfig`/`build_run_config`) + shared file-routing (`route_files`), so `ferric query` and `ferric mcp` can't drift — the persisted profile (ADR-029) is read once at launch (a running server picks up a re-calibration only on restart, deliberate per ADR-046); **T-3603** JSON-RPC message types + newline-delimited stdio framing (stdout = protocol frames only); **T-3604** `initialize` + `tools/list`; **T-3605** the `tools/call` handler (`McpServer`/`Executor`, `isError:true` on loop/provider failure without crashing the serve loop); **T-3606** `McpArgs` + `Command::Mcp` + `run_mcp` (one provider + one tokio `Runtime` built at launch, reused across calls; a real-subprocess stdio E2E). **T-3607** ADR-046 + docs + the reviewed external Production-Readiness-plan backlog. 16 new tests (incl. the structural schema-guarantee test, error-then-success-same-session recovery, and the `ferric mcp --mock` subprocess E2E). `cargo test --workspace` green; clippy `-D warnings` + fmt clean.
- **Completed:** 2026-07-03 (build/test phase)
- **Files modified:** crates/ferric-cli/src/{mcp.rs (new),query.rs,main.rs}, crates/ferric-cli/tests/cli.rs, crates/ferric-cli/Cargo.toml, Cargo.lock, decisions.md, README.md, agent-tasks/*
- **Commits:** 0f706ca (T-3601), acc0d6b (T-3602), e734938 (T-3603), 6654514 (T-3604), cd39ba9 (T-3605), c86bd9b (T-3606), T-3607 (this commit)

## T-3701/T-3702/T-3703/T-3704/T-3705/T-3706 (sprint 37)
- **Description:** **Streaming inference — fills ADR-003's reserved `complete_stream` extension point.** User-chosen sprint focus, framed as "a base architectural choice." The core design tension: `ConstrainedJson` (the flagship path) returns every turn's completion — including the final `task_complete` answer — as ONE opaque JSON object; raw token deltas of that aren't human-readable. Solved with a small incremental scanner recognizing exactly two signals: an early `"tool":"<name>"` activity signal (reusing ADR-016's field-ordering discipline for a new purpose) and, only for `task_complete`, the live-decoded `args.summary` characters — handling JSON string-escape sequences (incl. multi-byte `\uXXXX`) correctly across arbitrary chunk boundaries. Tasks: **T-3701** `StreamDelta` + `Provider::complete_streaming` with a default impl (every non-overriding provider — mock, mistral.rs — behaves identically to `complete()`, zero code, zero behavior change); **T-3702** `ConstrainedJsonScanner` (pure, `crates/ferric-provider/src/stream_scan.rs`), including a regression pinning the false-positive-safety argument and exhaustive escape-boundary tests; **T-3703** `OpenAiProvider::complete_streaming` — real SSE accumulation via `Response::chunk()` (discovered mid-build to need NO cargo feature or extra dependency, simpler than the originally-planned `bytes_stream()`+`futures_util::StreamExt`), a pure `feed_line`/`finish` accumulator, and a hand-rolled `tokio::net::TcpListener` fake-server E2E test (`Connection: close` framing, no `Content-Length`, since SSE bodies are unbounded); **T-3704** `RunArgs.stream_sink` threaded through the loop + `complete_streaming_with_backoff` (mirrors the existing retry policy; a retryable mid-stream error retries fresh, never replaying a failed attempt's deltas); **T-3705** `ferric query --stream` (opt-in, prints `Text` live to stdout, `ToolNamed` as a stderr activity line, skips the final echo when streaming already displayed the answer); **T-3706** ADR-047 + docs. Two foreground critic rounds (plan + build) caught and fixed 10 concerns before/during build, incl. the exact `\uXXXX` multi-byte escape-withhold rule, the false-positive-safety justification, retry-duplication coverage, and the Cargo.toml feature scoping (`reqwest`'s `stream` feature was added then REMOVED once `chunk()` proved unnecessary; `tokio`'s `net`/`macros`/`io-util` are dev-dependency-scoped, test-only). 25 new/hardened tests across `ferric-provider`, `ferric-loop`, and `ferric-cli`; `cargo test --workspace` + `--features backend-openai` green; clippy `-D warnings` (both feature sets) + fmt clean.
- **Completed:** 2026-07-03 (build/test phase)
- **Files modified:** crates/ferric-provider/src/{types.rs,traits.rs,stream_scan.rs (new),openai.rs,lib.rs}, crates/ferric-provider/Cargo.toml, crates/ferric-loop/src/{run.rs,backoff.rs}, crates/ferric-loop/tests/{common/mod.rs,backoff_tests.rs,streaming_tests.rs (new)}, crates/ferric-cli/src/{query.rs,mcp.rs}, crates/ferric-cli/tests/cli.rs, Cargo.toml, Cargo.lock, decisions.md, README.md, agent-tasks/*
- **Commits:** 064ad16 (T-3701), a555704 (T-3702), 6e99a22 (T-3703), 61d7f58 (T-3704), 7fce415 (T-3705), T-3706 (this commit)

## T-3801 (sprint 38)
- **Description:** **Persistent config foundation.** New `crates/ferric-cli/src/config.rs`: `Config` (serde `Deserialize`, every field `Option<T>`, the bounded ADR-005-safe field list — `backend`/`model_dir`/`model_file`/`model`/`api_base`/`api_key`/`params_b`/`quant`/`family`/`ctx`/`temperature`/`max_ring`/`profile_dir`/`stream`); `project_config_path` (`<workspace>/.ferric/config.toml`); `user_config_path_from(env: &impl Fn(&str) -> Option<String>)` — the plan-critic's C-003 fix, an env-injectable core (Windows `APPDATA` → XDG `XDG_CONFIG_HOME` → `.config` `HOME`-fallback → `None`) so every branch is unit-tested without touching real process env, with `user_config_path()` as the one-line real wrapper; `LoadedConfig { config, diagnostics: Vec<String> }` — the plan-critic's C-004 fix, malformed-TOML diagnostics returned as testable data (mirrors `RunConfig::prompt_composition_error`) rather than a bare `eprintln!`; `load_layered_from`/`load_layered` (project wins over user wins over `None`; a malformed layer degrades to `None` + one diagnostic, never panics). `BackendArg` gained `Serialize`/`Deserialize` + `#[serde(rename_all = "kebab-case")]` (matches clap's own lowercase spelling). `toml` added as a direct `ferric-cli` dependency (already a workspace dep via `ferric-bench`). Not yet wired into `query.rs`/`mcp.rs` (T-3803/T-3805) — `#![allow(dead_code)]` on the module is temporary, removed once those land. 16 new tests.
- **Completed:** 2026-07-04 (build phase)
- **Files modified:** crates/ferric-cli/src/config.rs (new), crates/ferric-cli/src/backend.rs, crates/ferric-cli/src/main.rs, crates/ferric-cli/Cargo.toml
- **Commit:** `8aeb2dd`

## T-3802 (sprint 38)
- **Description:** **Mechanical clap-default removal for `ferric query` (behavior-preserving, split from config wiring per plan-critic C-002).** `params_b`/`quant`/`family`/`ctx`/`temperature`/`profile_dir` on `QueryArgs` lose their clap `default_value_t`/`default_value`, becoming bare `Option<T>`; `run_query`'s `build_run_config` call site applies the exact same hardcoded defaults via `.unwrap_or(...)`/`.unwrap_or_else(...)` — no config file involved yet (T-3803). New regression test `cli::query_defaults_unchanged_after_clap_type_change`: a no-flags `--mock` run's `policy_selected` trace event still shows `tier: nano` / `max_output_tokens: 512` (the default `--params-b` 1.2's tier), proving the refactor alone changed nothing — isolating this task's correctness from T-3803's config-precedence logic.
- **Completed:** 2026-07-04 (build phase)
- **Files modified:** crates/ferric-cli/src/query.rs, crates/ferric-cli/tests/cli.rs
- **Commit:** `6022c98`

## T-3804 (sprint 38)
- **Description:** **Mechanical clap-default removal for `ferric mcp` (behavior-preserving), the same shape as T-3802.** `McpArgs`' matching six fields lose their clap defaults for bare `Option<T>`; `McpServer::launch`'s `build_run_config` call site applies the identical hardcoded defaults. New regression test `mcp::launch_defaults_unchanged_after_clap_type_change`: an all-`None` `McpArgs` (`--mock`, isolated tempdir workspace) resolves `Tier::Nano` / 512 max output tokens via `McpServer::launch` in-process, mirroring `cli::query_defaults_unchanged_after_clap_type_change`.
- **Completed:** 2026-07-04 (build phase)
- **Files modified:** crates/ferric-cli/src/mcp.rs
- **Commit:** `fecbe7a`

## T-3803 (sprint 38)
- **Description:** **Config loading + precedence resolution for `ferric query`, plus the C-001 `model_key` fix.** `run_query` now calls `crate::config::load_layered(&workspace_root)` once and resolves every relevant field as `cli_arg.or(config.field).unwrap_or(hardcoded_default)`: the six from T-3802, plus `BackendOpts`' `backend`/`model_dir`/`model_file`/`model`/`api_base`/`api_key` (merged IN PLACE on the owned `args.backend_opts` so `drive_real`'s `create_provider` call sees the same config-resolved values, not just this function's own `RunConfigArgs`), plus `max_ring`/`stream`. **A discovered, necessary companion fix**: `BackendOpts.backend` itself carried a clap `default_value = "mistral"` (unlike its 8 sibling fields, which the research report correctly noted as already bare `Option<T>`) — the exact same masking bug the plan-critic's C-001 flagged for `model_key`, just for the backend selector. Fixed by making `backend: Option<BackendArg>` (no clap default) with `.unwrap_or(BackendArg::Mistral)` applied at each of its 4 call sites (`create_provider`, `query.rs`, `mcp.rs`, and two `toolbench_cmd.rs` matches whose behavior is otherwise unchanged). **C-001 itself**: `model_key` is now derived from the POST-merge `args.backend_opts.model`/`model_file` (already config-resolved above), not raw CLI args — proven by the new `cli::config_only_model_still_resolves_profile` test (a config-only `model` + a persisted `calibrated_ring: 0` record: without the fix this test fails, since profile lookup would be silently skipped). **C-004**: `LoadedConfig.diagnostics` are both `eprintln!`'d and traced as a `Note` (proven by `cli::malformed_config_traced_as_note`) — testable data, not a bare unasserted print. Also added `cli::config_file_sets_default_without_flag` / `cli::cli_flag_overrides_config_file` (the core precedence proof, using `params_b` per the test-plan's C-007 scope note: `--mock` never reads `BackendOpts` fields, so `params_b` — a `ModelProfile`/tier-affecting field observable via the trace — is the right CLI-observable probe). 4 new tests.
- **Completed:** 2026-07-04 (build phase)
- **Files modified:** crates/ferric-cli/src/{query.rs,backend.rs,toolbench_cmd.rs}, crates/ferric-cli/tests/cli.rs
- **Commit:** `aeaec58`

## T-3805 (sprint 38)
- **Description:** **Config loading + precedence resolution for `ferric mcp`, the same shape (and the same C-001 fix) as T-3803.** `McpServer::launch` takes `&McpArgs` (not owned), so the merge builds a local `backend_opts` clone rather than mutating in place — that clone is what both `RunConfigArgs` and `build_real_provider` use, so a config-resolved model/backend actually reaches the real provider. `model_key` is derived from the merged `backend_opts`, not `args.backend_opts` directly. Malformed-config diagnostics are `eprintln!`'d at launch (matching the existing `prompt_composition_error` treatment) but deliberately NOT traced as a `Note` here — unlike `ferric query`, no trace sink exists yet at launch time (each `tools/call` opens its own), and writing the diagnostic into every subsequent call's trace would spam rather than inform; this is a considered deviation from the test-plan's literal `mcp::malformed_config_traced_as_note` case, noted in `sprint-meta.md`. 3 new tests (`launch_config_file_sets_default_without_flag`, `launch_cli_flag_overrides_config_file`, `launch_config_only_model_still_resolves_profile`) mirroring T-3803's CLI-level tests via in-process `McpServer::launch`. `cargo test --workspace` green; clippy `-D warnings` clean on default, `backend-openai`, and `backend-mistralrs` feature sets; fmt clean.
- **Completed:** 2026-07-04 (build phase)
- **Files modified:** crates/ferric-cli/src/mcp.rs
- **Commit:** `90ba8b0`

## T-3806 (sprint 38)
- **Description:** **`Animus.md` — a project-root, freeform, user-authored instructions file (the user's own framing: "much like CLAUDE.md but for Animus") read and folded into the system prompt.** New pure `fold_animus_md(existing: Option<&str>, animus_md: &str) -> String` (`query.rs`): appends `Animus.md`'s content as a distinct, clearly-delimited block after whichever base prompt already exists (oovra-composed, or `DEFAULT_SYSTEM_PROMPT` when absent) — deliberately NOT forced into oovra's versioned element system, which is the wrong shape for unversioned freeform prose (per the research report). Read via plain `std::fs::read_to_string` — no parsing, no schema; trusted context (the workspace owner's own words), not Ornstein-quarantined. `run_query` wires this in right after `build_run_config`; `McpServer::launch` reuses the same `fold_animus_md` helper. **C-005 (plan-critic, narrowed)**: presence is traced as a `Note` (`cli::animus_md_present_traces_note`); absence stays untraced, matching the existing precedent that the ordinary default path (e.g. no `prompts_dir` configured) is untraced — every other CLI test (none create an `Animus.md`) already proves the absent case is unchanged. For `ferric mcp`, presence is `eprintln!`'d at launch instead of `Note`-traced (no sink exists yet at launch time; matches T-3805's same treatment of malformed-config diagnostics) — a considered deviation from the test-plan's literal mcp-side Note-tracing case, noted here rather than forcing a per-`tools/call` repeat. 5 new tests (2 unit on `fold_animus_md`, `cli::animus_md_folds_into_prompt`, `cli::animus_md_present_traces_note`; the absent case is covered for free by every existing CLI test). `cargo test --workspace` green; clippy `-D warnings` clean (default + `backend-openai`); fmt clean.
- **Completed:** 2026-07-04 (build phase)
- **Files modified:** crates/ferric-cli/src/{query.rs,mcp.rs}, crates/ferric-cli/tests/cli.rs
- **Commit:** `ef324aa`

## T-3807 (sprint 38)
- **Description:** **ADR-048 + docs.** Recorded the config-precedence design (CLI > project > user > default), the bounded-field ADR-005 rationale, the `Animus.md` trust-tier decision, the ADR-010 non-interaction note, and — the most notable part — the masking-hazard bug class the plan-critic caught (`model_key`'s C-001 fix) and its SECOND, self-discovered instance (`BackendOpts.backend`'s leftover clap default), recorded so future config-surfaced fields get checked for the same class of bug. README Status bumped to sprint 38 + a new Sprint 38 timeline entry; the Production-Readiness Roadmap's "Persistent config" bullet marked DONE; the Sprint 38 backlog section rewritten from its in-progress checklist to a completed summary (matching sprints 36/37's precedent).
- **Completed:** 2026-07-04 (build phase)
- **Files modified:** decisions.md, README.md, agent-tasks/agent-tasks.md, agent-tasks/completed-tasks.md
- **Commit:** `7b99bd7`

## T-3901 (sprint 39)
- **Description:** **New trace event `Event::SessionPrompt` + `SessionStart.resumed_from` — the biggest lossless-replay gap, closed.** `crates/ferric-trace/src/event.rs` gains `Event::SessionPrompt { system, user, media }` (written once per session, right after `PolicySelected`/`PromptComposed`, before `TurnStart(0)`) — closes the gap where the ORIGINAL system+user prompt text was never recorded anywhere in the trace, only derived metadata (`PromptComposed`'s lineage, `PromptAssembled`'s char counts). `SessionStart` gains `resumed_from: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`, additive) — per the plan-critic's C-002 fix, stores the ORIGINAL session's `session` id string (not a file path — stable even if files move; resume-of-a-resume chains need no special handling since `replay()`, landing in T-3903, only ever reads the ONE target file). Every existing caller writes `resumed_from: None` for now (T-3904 makes it conditional once `RunArgs.resume` exists). `TurnEnd` gains a placeholder `truncated: false` field (its real value wired in T-3902) purely to keep this commit's diff to the trace-format additions, not yet the behavior that populates it. Updated `ferric trace cat`'s renderer (`trace_cmd.rs`) for both new/changed fields (shows `resumed from <id>` and a `TRUNCATED` marker) — derived-view parity, not just compile-fixing. 8 new/updated tests in `ferric-trace` (round-trips + backward-compat fixtures for both `resumed_from` and pre-existing lines with neither new field) plus a `session_prompt` entry added to the existing `kinds()` golden-order test helper and its one exact-sequence assertion (`loop_core.rs`).
- **Completed:** 2026-07-04 (build phase)
- **Files modified:** crates/ferric-trace/src/{event.rs,lib.rs}, crates/ferric-loop/src/run.rs, crates/ferric-cli/src/trace_cmd.rs, crates/ferric-tools/tests/guarded_traced_execution.rs, crates/ferric-provider/tests/mock_loop_skeleton.rs, crates/ferric-loop/tests/{common/mod.rs,loop_core.rs}
- **Commit:** `1a628cd`

## T-3902 (sprint 39)
- **Description:** **Extend existing events for full turn fidelity — the terminator's `ToolCall` + `TurnEnd.truncated`'s real value.** `run.rs`'s dispatch loop now writes an `Event::ToolCall` for the terminator (`task_complete`) call too, in EVERY protocol — never dispatched/executed, just traced — closing the gap where a `NativeTools` session's summary was recorded nowhere in the trace (`ConstrainedJson`/`TextXml` already carried it in that turn's raw `TurnEnd.text`, so this is additionally redundant-but-harmless there, and keeps behavior uniform across protocols). **Placement matters (plan-critic C-003):** the trace-write sits INLINE at the exact position of the existing `continue` inside the per-call dispatch loop, so trace order stays identical to `actions`' original (model-emission) order even when the terminator is mixed among other calls in the same turn — this is the one place a naive "log it after we know we're terminating" implementation would have silently corrupted order for exactly the multi-tool-call-per-turn case T-3903's replay depends on. `TurnEnd.truncated` (added as a placeholder in T-3901) now gets its real value from `completion.truncated`. Two pre-existing tests conflated "traced" with "dispatched" (`terminator_tests::task_complete_terminates`, `grammar_loop::textxml_terminator_intercepted`) — both asserted "no `tool_call` event for the terminator," which was actually testing non-dispatch; fixed to assert the real invariant (`tool_call` now present, `tool_result` still absent — the terminator is traced but never routed through the registry). `cargo test --workspace` green; clippy/fmt clean.
- **Completed:** 2026-07-04 (build phase)
- **Files modified:** crates/ferric-loop/src/run.rs, crates/ferric-loop/tests/{terminator_tests.rs,grammar_loop.rs}
- **Commit:** `3506ac2`

## T-3903 (sprint 39)
- **Description:** **`ferric-loop::replay` — reconstruct `ReplayedState` from an interrupted trace.** New `crates/ferric-loop/src/replay.rs`: `ReplayedState { messages, turns, last_text, protocol, source_session }` + `ReplayError { Trace, MissingSessionPrompt, AlreadyStopped(String) }`. `replay(path)` runs a first pass rejecting any trace with a `SessionEnd` anywhere (`AlreadyStopped` — a session that reached ANY stop reason isn't "interrupted"), then a second pass reconstructing turn-by-turn. **A real design correction found only during implementation**: `TurnEnd` is written BEFORE dispatch in `run()` (not after), so "this turn has a `TurnEnd`" does NOT prove its tool calls/guard checks/results finished — a crash mid-dispatch leaves a `TurnEnd` on disk with an incomplete tail. The locked plan's EARS clause only anticipated "`TurnStart` with no matching `TurnEnd`" as the dangling case; the ACTUAL correct signal is stricter: a turn is only committed once a LATER `TurnStart` confirms its dispatch fully ran — buffered in a `PendingTurn` (its `ToolCall`s, `ToolResult`s, and which guards warned) and finalized only then. This is a strict superset of the locked EARS clause (still discards the literal "no TurnEnd" case, plus the additional "has TurnEnd but unconfirmed dispatch" case the plan's simpler wording didn't drill into) — noted here as an honest build-time refinement, not a contradiction. Extracted `run.rs`'s five distinct inline nudge templates (no-action, truncation-retry, repetition-warn, no-progress-warn, failure-warn — genuinely different wording each, never collapsed into one generic formatter per C-007) plus `result_message` into `pub(crate)` functions `run()` and `replay()` both call, so they can't drift apart. 12 new tests (co-located `#[cfg(test)]` in `replay.rs`, not `tests/`, since the extracted helpers are `pub(crate)` and only visible within the crate) covering: a clean multi-turn `ConstrainedJson` reconstruction, `NativeTools` multi-tool-call order preservation, the terminator MID-turn ordering proof (C-003), both guard nudges (proving they're distinct, C-007), the `TextXml` parse-error fallback (C-005), the truncation retry, TWO dangling-turn variants (no `TurnEnd` at all, and — the stricter refinement — a `TurnEnd` with unconfirmed dispatch), and both error paths. `cargo test --workspace` green (33 `ferric-loop` unit tests, up from 21); clippy/fmt clean.
- **Completed:** 2026-07-04 (build phase)
- **Files modified:** crates/ferric-loop/src/{replay.rs (new),lib.rs,run.rs}
- **Commit:** `ed36431`

## T-3904 (sprint 39)
- **Description:** **Thread `ReplayedState` into `RunArgs`/`run()`; relax `prompt` to `Option<&str>`.** `RunArgs` gains `resume: Option<ReplayedState>`; `run()`'s `prompt` parameter becomes `Option<&str>` (mechanical ripple through `run_with_provider`/`drive_mock`/`drive_real`/`run_one` — every existing call site wraps its `&str` in `Some(...)`, confirmed byte-identical by every pre-existing test passing unchanged). `run()`'s opening now branches: `resume: None` builds `[system, user]` fresh, writes `SessionPrompt`, `resumed_from: None` — byte-identical to before this sprint; `resume: Some(replayed)` seeds `messages`/`turns`/`last_text` from it, writes `SessionStart.resumed_from`, and skips `SessionPrompt` (a resumed session has no new initial prompt — its own lives in the session it resumed from). A resumed run MAY also carry one extra user-supplied nudge (`prompt: Some(p)` even while resuming), appended after the replayed history. `resume: None` + `prompt: None` now returns a `FerricError::InvalidInput` rather than panicking — a state the CLI layer (T-3905) is responsible for never producing. `ferric mcp`'s `run_one` passes `resume: None` unconditionally (`--resume` is out of scope for MCP, ADR-046's launch-time-fixed design). 4 new tests in `crates/ferric-loop/tests/resume_tests.rs`, including test-critic C-010's genuine round-trip (`real_run_then_replay_then_resume_reaches_task_complete`): a REAL `run()` call to `TaskComplete`, its real trace file truncated (drop the trailing `SessionEnd` line, simulating a kill), `replay()`d, then a SECOND real `run()` call resumes it and reaches `TaskComplete` again — the strongest proof `run()`'s actual emission and `replay()`'s assumptions haven't drifted, since every other replay test uses a hand-built fixture. `cargo test --workspace` green (including `--features backend-openai`); clippy/fmt clean.
- **Completed:** 2026-07-04 (build phase)
- **Files modified:** crates/ferric-loop/src/run.rs, crates/ferric-loop/tests/{common/mod.rs,backoff_tests.rs,streaming_tests.rs,resume_tests.rs (new)}, crates/ferric-cli/src/{query.rs,mcp.rs}
- **Commit:** `12cf41a`

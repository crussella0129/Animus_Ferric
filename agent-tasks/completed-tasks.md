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

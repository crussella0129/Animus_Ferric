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
- **Commit:** (see git log for T-209)

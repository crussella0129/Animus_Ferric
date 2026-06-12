# Sprint 2 — Animus Ferric: Prompts, Unified Action Grammar, Calibration

## Context

s1 proved the engine (real-GGUF L0 smoke passed) and exposed the next problem: the 1B *described* `task_complete` in prose instead of calling it. s2 makes Ferric's behavior taught and measured: system prompts become versioned, composed oovra elements with genealogy in the trace; the action space becomes ONE llguidance JSON-Schema (every tool + task_complete as anyOf branches) so malformed actions are grammatically impossible; and the L0–L6 ladder ports as `ferric bench`, producing `measured_level` calibration that closes ADR-006's loop. The HTTP escape valve moves s2→s3 (spec research-complete and banked; ADR-017). Research pre-resolved the unknowns: oovra's lib API is on main at rev `378abea` (pin by rev); mistralrs `finish_reason` is a `String` (`"length"` = truncated); llguidance 1.7.6 accepts anyOf but REJECTS oneOf; and workspace serde_json needs `preserve_order` or generated branches would emit `args` before `tool`, defeating early branch commitment.

## Build plan

**Wiring (T-201):** new default crates `ferric-prompt` (oovra git rev-pinned + ferric-core) and `ferric-bench` (serde/serde_json/toml/regex/tempfile + ferric-core/trace). Workspace serde_json gains `preserve_order`. Default graph stays mistralrs/tokio-free; aarch64 gate covers the new crates; CI unchanged.

**16 elementary tasks (T-201..T-216):**
- T-202 `ActionProtocol {NativeTools, UnifiedGrammar}` in ferric-core + `RunPolicy.max_output_tokens` (seeds 512/768/1024/1536/2048/2048, snapshot-pinned).
- T-203 trace events `PolicySelected` (tier/protocol/budgets) + `PromptComposed` (lineage) + render arms.
- T-204 `Completion.truncated` plumbed from `finish_reason == "length"` (free-function tested).
- T-205 `move_path` + `make_dir` NANO tools (both endpoints boundary-checked; unblocks bench L1/L2).
- T-206 grammar module: `action_schema(&[ToolDescriptor])` → x-guidance/anyOf/const-discriminator schema (tool-first property order pinned by golden test; "oneOf" asserted absent) + `parse_action` (completion text → synthesized ToolCall).
- T-207 loop integration: protocol-driven requests (UnifiedGrammar = constraint-only, tools empty — ADR-010 invalid state unrepresentable); grammar actions normalize into the SAME dispatch path (terminator, repetition guard, permission events identical); results framed as user-role `[tool_result for X]` messages; NativeTools stays byte-identical to s1.
- T-208 truncated action → nudge once → `StopReason::TruncatedAction`; unparseable (non-truncated) → existing empty-completion path.
- T-209 ferric-prompt: in-repo `prompts/` atoms (role, workspace rules, per-protocol teaching, terminator teaching), `compose_system_prompt(lib, tier, protocol) → ComposedPrompt {text + id/version lineage}`; caller falls back to DEFAULT_SYSTEM_PROMPT.
- T-210 query wiring: `--protocol native|grammar`, `--prompts-dir`, policy-driven max_tokens.
- T-211..T-215 ferric-bench: embedded TOML L0–L6 specs (deny_unknown_fields; Ferric tool names), spawn-self runner (`current_exe()`, child always `query` — recursion structurally impossible; timeout kill; `--keep-workspace`), trace-derived verification (completed = !timeout ∧ exit0 ∧ expectations ∧ tools ∧ post_verify ∧ clean terminator; plan_steps null — flagged not faked), append-only results.jsonl + `model_profiles.json` (measured_level = highest completed level; tier_from_params vs tier_from_measured), `ferric bench` subcommand with `--mock` as the CI-runnable self-test.
- T-216 real-model gates: l0_smoke × BOTH protocols + calibration sweep (1B L0–L4 × both protocols; Qwen-7B best-effort) committed as the sprint's empirical record; ADR-015..019 recorded.

**New ADRs:** 015 ActionProtocol (+truncation semantics); 016 oovra rev-pin + allowlist growth (incl. preserve_order rationale); 017 HTTP→s3 amendment; 018 output-token budgets; 019 calibration pipeline (bench is the sole producer of measured_level; tier changes only with a committed measurement diff).

## Test plan

- **Unit:** snapshot+serde (T-202); event round-trips (T-203); finish_reason mapping (T-204); move/mkdir incl. cross-boundary deny + missing-source error (T-205); schema golden (anyOf present, oneOf absent, tool-before-args, additionalProperties both depths) + parse_action garbage rejection (T-206); compose all tier×protocol pairs + lineage + protocol-exclusive teaching (T-209); spec parsing + unknown-field rejection (T-211); verification verdict matrix + append-not-truncate + measured_level selection (T-213/214).
- **Integration (bulk):** mock-driven UnifiedGrammar loop tests — grammar-JSON text completions through the full path (golden event order incl. PolicySelected/ConstraintApplied, terminator-via-grammar, truncation ×1/×2, unparseable fallback, repetition on identical actions, request-shape recording); native-mode regression suite unchanged; bench harness mock e2e via `CARGO_BIN_EXE` (fixture spec matching the built-in mock script, timeout fixture, keep-workspace).
- **E2E (manual, release, ADR-009):** l0_smoke native + grammar variants; calibration sweep recorded into `benchmarks/` and sprint-tests. Empirically answers: does the grammar fix the prose-terminator failure, and does grammar mode regress Qwen's native-format quality?
- Known impossible without a model: actual llguidance mask enforcement (smoke is the schema-compile acceptance gate), real finish_reason, prompt-quality deltas, all calibration numbers.

## Key risks
Grammar-mode quality regression on tool-format-trained models (both protocols stay first-class; data decides defaults); bench wall-time on 7B (spec timeouts, --level selection, 1B-first); debug-binary spawn-self (warn under cfg!(debug_assertions)); llguidance runtime schema rejection (generator restricted to the verified construct set; smoke gates before sweeps).

## After approval (sprint-loop protocol)
Write sprints/s2/sprint-plans/{build,test}-plan.md per schemas → plan-critic → finalize-plan.sh → Build (per-task commits, gates) → Test (incl. real-model smoke ×2 + calibration sweep) → Loop.

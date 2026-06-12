# Sprint 2 Research Report — Prompts, the Unified Action Grammar, and Calibration

## Decisions Reviewed

All fourteen ADRs reviewed; none violated. Touched this sprint:

- **ADR-010 (constraint×tools exclusivity)** — *vindicated by source research*: mistralrs 0.8.1 does NOT guard the combination (independent request fields), while llama.cpp's server rejects it. Our harness-side invariant is the only guard on the in-process path. s2 evolves the mechanism per the ADR's own forward note: an `ActionProtocol` enum (NativeTools | UnifiedGrammar) makes the invalid state *unrepresentable* — the unified grammar uses constraint-only requests with tool semantics taught in the system prompt.
- **ADR-014 (capability roadmap)** — one sequencing amendment proposed: the **HTTP escape-valve backend moves s2 → s3**. Its spec is research-complete (artifact ready to execute), but s2's core (prompts + grammar + bench + calibration) is a full sprint; HTTP adds a transport layer with its own test burden and nothing downstream of s2 depends on it. Circuit-breaker compaction and stale-config migration remain s3+ as already recorded (no long-session/config layer exists yet).
- **ADR-002 (trace)** — additive event this sprint: `PolicySelected` (tier, protocol, budgets) closing the benchmark-parity gap (tier/grammar status currently absent from traces).
- **ADR-004 (allowlist)** — grows by: `oovra` (git dependency — not on crates.io; remote public and current) and `toml` (ferric-bench spec files; already transitively present via oovra). Default-build aarch64 invariant unaffected (both pure Rust).
- **ADR-006 (scale function)** — s2 closes its loop: `ferric bench` produces `measured_level` per model, feeding the already-implemented bidirectional override. Also extends RunPolicy with the per-turn output-token budget (s1 finding).
- **ADR-009 (real-GGUF gate)** — in force: the loop/provider/grammar changes here require traced real-model runs; the calibration sweep itself becomes part of the gate.
- ADR-011/012/013 untouched (command surface unchanged; MCP is s3; no new non-Rust residue — oovra and toml are pure Rust).

## 1. Sprint Goal

Make Ferric's behavior *taught and measured* instead of hardcoded and assumed. Four strands: (1) **ferric-prompt** — per-tier/per-protocol system prompts composed from a versioned oovra element library, with composition genealogy in the trace; (2) **the unified action grammar** — one llguidance JSON-Schema constraint covering the whole action space (every tool + task_complete), making malformed actions impossible and directly attacking s1's observed failure (the 1B *describing* task_complete instead of calling it); (3) **ferric-bench** — the L0–L6 ladder ported as TOML specs + a `ferric bench` runner producing results.jsonl and `measured_level` calibration for the fleet; (4) supporting fixes — `move_path`/`make_dir` tools (L1/L2 are unrunnable without them; lineage-proven NANO ops), per-turn output-token budgets in RunPolicy, and the `PolicySelected` trace event.

## 2. Existing Code Survey

| File | Relevance | Notes |
|------|-----------|-------|
| `crates/ferric-loop/src/run.rs` | high | Gains UnifiedGrammar mode: constraint-only requests, whole-completion Action parse, finish-reason handling, protocol-driven request building. |
| `crates/ferric-provider/src/types.rs` | high | `Completion` gains truncation signal (finish_reason); `SamplingParams.max_tokens` becomes policy-driven. |
| `crates/ferric-provider/src/mistralrs.rs` | high | Maps response finish_reason; everything else unchanged (Constraint plumbing already 1:1). |
| `crates/ferric-core/src/scale.rs` | high | RunPolicy gains `max_output_tokens` per tier; `Protocol::ConstrainedJson` now actually selects UnifiedGrammar in the loop. |
| `crates/ferric-trace/src/event.rs` | med | Additive `PolicySelected { tier, protocol, budgets }` event (benchmark parity). |
| `crates/ferric-tools/src/builtin/` | med | +`move_path`, +`make_dir` (NANO, lineage-proven; unblock L1/L2). |
| `crates/ferric-cli/src/query.rs` | med | Wires composed prompts + protocol selection; gains `--protocol` override flag for A/B runs. |
| `crates/ferric-cli/tests/l0_smoke.rs` | med | Template for bench runner's spawn-the-binary pattern; re-run gates this sprint. |
| oovra `src/{library,render,element,header}.rs` | high | The exact lib API Ferric consumes (signatures verified — see artifact). |
| Animus `tests/benchmarks/*.yaml` + `scripts/run_benchmark.py` | high | The ladder + run protocol + results schema to port faithfully (see artifact, incl. field-level mapping and gaps). |
| `~/.cargo/.../llguidance-1.7.6/src/json/compiler.rs` | high | Source-verified: anyOf yes, oneOf REJECTED, $defs/$ref yes, property-order emission. |
| `~/.cargo/.../mistralrs-core-0.8.1/src/{request.rs,pipeline/llg.rs}` | high | Constraint::JsonSchema → llguidance mask over the whole completion; NO exclusivity guard (harness's job). |

## 3. External Sources

- [llguidance JSON-Schema docs](https://github.com/guidance-ai/llguidance/blob/main/docs/json_schema.md) — support matrix; anyOf-not-oneOf; x-guidance options. Cross-verified against the lockfile-resolved 1.7.6 source.
- [llama.cpp server README](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md) + `server-common.cpp` — exact response_format nesting, exclusivity errors, /health, usage fields (s3 HTTP valve, spec banked).
- [llama.cpp function-calling docs](https://github.com/ggml-org/llama.cpp/blob/master/docs/function-calling.md) — native vs generic tool handlers, arguments-as-string.
- [mistral.rs PR #899](https://github.com/EricLBuehler/mistral.rs/pull/899) — llguidance integration provenance.
- [oovra](https://github.com/crussella0129/oovra) — the prompt-composition dependency (git dep until crates.io v0.3).

(Artifacts carry the full source lists and verified API quotes.)

## 4. Risks, Unknowns, Dependencies

- **Risk — grammar mode quality on trained-for-tools models:** the from-token-zero mask bypasses template-native tool formats some models were trained on; quality could *drop* for e.g. Qwen-2.5-Coder vs native mode. Mitigation: ActionProtocol keeps both modes; `ferric bench --protocol` A/Bs them; the calibration sweep decides per-model defaults with data, not belief.
- **Risk — truncated actions:** `finish_reason == "length"` is the one malformed-action case the grammar can't prevent. The loop must treat it as a failed action (nudge/retry once, then stop) and the new per-tier `max_output_tokens` must leave headroom over typical action sizes.
- **Risk — oovra git-dep branch:** local checkout sits on `feat/create-redesign-and-gui-bootstrap`; the lib API verified may not be on `main`. Must confirm/pin the right rev (or merge to main) before CI depends on it.
- **Risk — bench wall-time:** a full L0–L6 sweep on the 7B at 4–10 tok/s CPU could run hours. Mitigation: per-level timeouts from the specs, `--level` selection, 1B-first calibration, release profile mandatory (s1 lesson, encoded in the runner).
- **Unknown — does UnifiedGrammar fix the s1 terminator failure?** The lineage says yes (grammar_mode=full + task_complete worked on NANO); the calibration sweep answers it empirically this sprint.
- **Unknown — mistralrs finish_reason surface:** the response's finish/stop reason field shape in 0.8.1 needs source confirmation at build time (Choice struct).
- **Dependency:** oovra (git), toml (specs), existing fleet GGUFs; no new non-Rust residue.

## 5. Recommended Approach

**Primary — four components, ~14 elementary tasks, HTTP valve deferred to s3:**

1. **ferric-prompt crate**: oovra git dep; in-repo `prompts/` element library (role, workspace rules, native-tools protocol, unified-grammar protocol, terminator teaching — per tier where wording differs); `compose_system_prompt(lib, tier, protocol)`; composition genealogy traced. The loop's DEFAULT_SYSTEM_PROMPT becomes the fallback when no library is present.
2. **Unified action grammar**: `ActionProtocol` enum; schema generator (ToolDescriptors + task_complete → anyOf-of-const-branches, tool-first property order, whitespace_flexible:false, additionalProperties:false); loop UnifiedGrammar mode (constraint-only request, serde-parse whole completion → Action, dispatch through the same executor path); finish_reason plumbed through Completion; truncated-action handling.
3. **ferric-bench**: new crate + `ferric bench` subcommand; TOML L0–L6 specs (tool names mapped to Ferric's set); run protocol per the port spec (spawn release `ferric` binary); results.jsonl rows (nulls where Ferric has no source yet, e.g. plan_steps); calibration output (`measured_level` per model → model_profiles.json); `PolicySelected` trace event + `move_path`/`make_dir` tools to unblock L1/L2.
4. **Policy budgets**: `max_output_tokens` per tier in RunPolicy (snapshot-pinned), driving SamplingParams.
5. **Real-model gates (ADR-009)**: L0 smoke re-run (both protocols) + first calibration sweep of Llama-3.2-1B across L0–L4; Qwen-7B best-effort. This is also the empirical answer to whether the unified grammar fixes the prose-terminator failure.

**Alternative considered:** include the HTTP escape-valve backend in s2 as roadmapped. Rejected for this sprint: its research spec is complete and banked (zero re-research cost in s3), nothing in s2 depends on it, and the grammar/bench/prompt work already carries this sprint's real-model validation burden. Pulling it forward would trade calibration depth for transport breadth.

**Rationale:** s1 proved the engine; s2 makes it *honest* — prompts versioned and auditable (oovra genealogy in the trace), actions grammatically incapable of being malformed, and the tier table calibrated by measurement instead of parameter-count folklore. Every piece feeds the next sprint's surfaces (MCP/GECK/Docker in s3 all sit on a measured, grammar-disciplined loop).

## Artifacts

- `oovra-integration-spec.md` — verified lib API, dependency strategy (git dep + branch caveat), ferric-prompt sketch.
- `benchmark-port-spec.md` — faithful ladder + run protocol + results schema, Ferric metric mapping with gaps, port shape, calibration pipeline, L1/L2 tool gap.
- `action-grammar-http-spec.md` — llguidance 1.7.6 verified support matrix, mistralrs constraint mechanics, the concrete 4-action schema, ActionProtocol recommendation, and the complete (banked for s3) llama-server HTTP spec.

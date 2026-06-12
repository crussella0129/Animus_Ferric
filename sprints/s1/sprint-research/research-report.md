# Sprint 1 Research Report — First Real Backend + the Ferric Vision

## Decisions Reviewed

All nine ADRs from sprint 0 bear on this sprint; none is violated, two are extended:

- **ADR-001 (workspace, harness-owns-decoding, dual backends, FFI rejected)** — confirmed by the mistral.rs spec (in-process llguidance) and the deep-Rust survey (llama.cpp FFI remains rejected). Extended this sprint: new ADRs for command structure and integration shapes.
- **ADR-002 (JSONL trace source of truth)** — s1 adds the reserved event names (prompt assembly, constraint state) now that real content exists.
- **ADR-003 (async dyn-compatible Provider, Constraint carried per request)** — validated: mistral.rs `Constraint` maps 1:1 (JsonSchema/Regex/Lark). One amendment learned from the spec: **constraint and tools are mutually exclusive per request** (a custom constraint applies to the whole output and fights tool-call syntax) — recorded as ADR-010.
- **ADR-004 (CPU-first, dependency allowlist, aarch64 gate)** — confirmed; mistral.rs default build IS the CPU build; the s1 allowlist grows by exactly: mistralrs 0.8.1, tokio (it's already in mistralrs's tree), and nothing else.
- **ADR-005 (hardcoded security)** — extended by the OpenClaw security taxonomy: Ferric is the unified policy boundary regardless of invoking layer; never rely on a caller's exec filtering.
- **ADR-006 (pure scale function, config-fed)** — untouched; L0–L6 calibration unlocks once the benchmark harness ports (s2+).
- **ADR-007/008** — unchanged gates.
- **ADR-009 (MockProvider-only in s0; real-GGUF validation from s1)** — **takes effect this sprint**: every task touching provider/loop/constraint requires a traced real-GGUF run.

## 1. Sprint Goal

Two strands. **Engineering (s1 build):** ship the first real inference backend (mistral.rs in-process, CPU, local GGUF), a production agent-loop crate carrying the deferred lineage fixes (hash-all repetition guard, structured terminator, backoff), the first real CLI surface (`ferric query`), and the L0-smoke E2E — one real-GGUF run producing a valid trace and a correct file edit. **Direction (this report + ADRs):** absorb Charles's Ferric vision into the architecture — Animus Ferric as a standalone-first tool with no "chat" catch-all (a `query` mode for unstructured one-shots and a future `dev` Development Engine for structured multi-sprint work), integrable with OpenClaw via MCP, an amalgamation home for oovra/GECK/sprint-loops, with Docker/Nix as first-class capabilities, the Ornstein quarantine pattern for retrieval, tailnet-native remote access, and a maximally auditable ownership graph.

## 2. Existing Code Survey

| File | Relevance | Notes |
|------|-----------|-------|
| `crates/ferric-provider/src/traits.rs` + `types.rs` | high | The Provider trait the MistralRsProvider must implement; Constraint enum already llguidance-shaped (validated by spec). |
| `crates/ferric-provider/tests/mock_loop_skeleton.rs` | high | The loop shape s1 productionizes into a `ferric-loop` crate; also the L0-smoke template. |
| `crates/ferric-core/src/scale.rs` | high | RunPolicy drives loop budgets (max_turns, turns/step, tool caps) and protocol selection. |
| `crates/ferric-tools/src/registry.rs` | high | Execute chokepoint the loop dispatches through; sorted/capped tools_for_policy feeds ToolDescriptors. |
| `crates/ferric-trace/src/event.rs` | high | Gains s1 events: PromptAssembled, ConstraintApplied, TurnStart/End, RepetitionGuard (reserved names from ADR-002). |
| `crates/ferric-cli/src/main.rs` | med | Grows `query` subcommand; std-arg parsing likely graduates to clap now that the surface is real. |
| `crates/ferric-guard/src/checker.rs` | med | Loop must trace PERMISSION_CHECK decisions; no logic change expected. |
| Animus `tests/benchmarks/` (L0–L6 YAML specs) | high | The benchmark harness to port (s2) — L0 spec defines the s1 smoke's pass criteria shape. |
| Animus `scripts/run_benchmark.py` | med | Reference for trace-parsing verification logic (workspace state assertions). |
| oovra `src/element.rs` / `library.rs` / `render.rs` | med | The prompt-composition lib Ferric will depend on in s2 (per-tier system prompts). |
| GECK `geck_generator/core/profiles.py` | low | 21 profiles to absorb as `ferric init-project` templates in s3. |
| sprint-loops `open-harnesses/` scripts + particles | med | The five-phase protocol ferric-engine ports in s4–s7; the filesystem contract is the spec. |
| `~/.animus/models/*.gguf` (5-model fleet) | high | Llama-3.2-1B Q4_K_M (771 MB) is the s1 smoke model; Qwen2.5-Coder-7B is the quality check. |
| `decisions.md` ADR-001..009 | high | Reviewed above; ADR-010..014 proposed this sprint. |

(Detailed surveys in artifacts: `amalgamation-survey.md`, plus s0's lineage surveys remain valid.)

## 3. External Sources

- [mistral.rs](https://github.com/EricLBuehler/mistral.rs) + [docs site](https://ericlbuehler.github.io/mistral.rs/) — full integration spec extracted (crate 0.8.1, GgufModelBuilder, Constraint API, Windows CPU CI-verified, zero-network local loading); see `mistralrs-integration-spec.md`.
- [OpenClaw docs](https://docs.openclaw.ai/concepts/architecture) (+ MCP/skills/tailscale pages) — gateway architecture, MCP-first integration surface, Tailscale Serve identity-header auth; see `openclaw-integration.md`.
- [OpenClaw security taxonomy, arXiv 2603.27517](https://arxiv.org/abs/2603.27517) — 470 advisories; lessons adopted into ADR-005 extension and the Ornstein design.
- [NVlabs cuda-oxide](https://github.com/NVlabs/cuda-oxide) + Burn/CubeCL releases — the future pure-Rust GPU path; verdict "Rust down to the driver ABI"; see `deep-rust-feasibility.md`.
- [Tailscale tailscale-rs preview](https://tailscale.com/blog/tailscale-rs-rust-tsnet-library-preview) + bollard/gVisor/Nix dockerTools/CaMeL sources — substrate verdicts; see `docker-nix-tailscale.md`.

(Each artifact carries its own extended source list; the five bullets above are the load-bearing ones.)

## 4. Risks, Unknowns, Dependencies

- **Risk — mistralrs dep-tree weight:** several hundred crates, multi-minute builds, 30–80 MB binaries; also pulls hf-hub/reqwest (network-capable code in-tree even if unused). Mitigate: isolate behind a `backend-mistralrs` cargo feature in ferric-provider so core crates stay lean and the aarch64 check gate stays fast; `HF_HUB_OFFLINE=1` + `TokenSource::None` enforced in code.
- **Risk — constraint×tools exclusivity (ADR-010):** the loop must choose per turn: constrained-JSON action grammar OR native tool calling. s1 takes the simplest correct path — use mistral.rs **native tool calling with `strict: true`** (grammar-enforced argument JSON, per-model parsers) for tool turns, reserve `Constraint` for extraction turns. The "harness-owns-decoding" unified action grammar (single JSON schema covering tool choice + task_complete) is s2 work once L0 establishes the baseline.
- **Risk — CPU throughput on 7B (~4–10 tok/s):** L0 smoke uses the 1B (~20–50 tok/s); keep smoke prompts tiny; deterministic sampler for reproducibility (lineage H22).
- **Risk — aarch64 + mistralrs:** upstream doesn't CI linux-aarch64. Our gate is `cargo check` only; runtime verification on Pi/Orin is explicitly deferred and tracked.
- **Unknown — GGUF chat-template variance:** Llama-3.2/Qwen2.5 official GGUFs embed templates; the abliterated community GGUFs in the fleet may not. Fallback `.with_chat_template(path)` must be plumbed through config.
- **Unknown — model load/RSS on this machine:** estimates only (1–2 GB for 1B); the L0 smoke measures and records actuals in the test report.
- **Dependency — local fleet:** Llama-3.2-1B-Instruct Q4_K_M present at `~/.animus/models` (verified s0).
- **Dependency — vision items deliberately NOT in s1 build:** oovra dep (s2), GECK absorption (s3), ferric-engine port (s4–s7), `ferric mcp` (s2–s3), Docker/Nix capability layer + Ornstein (s3+), tailnet surface (s3+). Recorded as ADR-014 roadmap + backlog entries so they cannot evaporate.

## 5. Recommended Approach

**Primary — s1 builds the engine core; the vision lands as ADRs + sequenced backlog:**

1. **`ferric-loop` crate (new):** productionize the mock_loop_skeleton — turn loop under RunPolicy budgets; native tool calling (strict) for tool turns; `task_complete` structured terminator; hash-ALL-calls repetition guard; exponential backoff on retryable provider errors; every stage traced (new events: TurnStart/End, PromptAssembled, ConstraintApplied, RepetitionGuard, PermissionCheck).
2. **`MistralRsProvider`** in ferric-provider behind `backend-mistralrs` feature, per the integration spec (GgufModelBuilder, offline enforcement, usage→token counts, constraint mapping, force_cpu).
3. **`ferric query "<prompt>"`** — the first real CLI surface and the embodiment of the no-chat-catch-all decision: one-shot, workspace-scoped, policy-scaled, fully traced. (`ferric dev` is reserved for the ferric-engine sprint.)
4. **L0 smoke E2E:** temp workspace → `ferric query` with Llama-3.2-1B → assert correct file edit + valid trace (mock_loop_skeleton's assertions against a real model). Ignored-by-default cargo test (`--ignored` + env var pointing at the model) so CI stays model-free; run locally per ADR-009.
5. **New ADRs:** ADR-010 constraint×tools exclusivity; ADR-011 command structure (query/dev, no chat); ADR-012 OpenClaw integration = MCP stdio first + SKILL.md companion, WS peer deferred; ADR-013 ownership-graph boundaries (tree-sitter C core accepted as named boundary; NVIDIA = "Rust down to the driver ABI"; CubeCL/Burn-LM convergence target; cuda-oxide watch); ADR-014 capability roadmap (oovra s2 → GECK s3 → Docker/Nix+Ornstein s3+ → ferric-engine s4–s7 → tailnet surface).

**Alternative considered:** make s1 the "vision sprint" — build `ferric mcp` + command scaffolding + oovra integration first, defer real inference to s2. Rejected: every vision item ultimately rides on a working local-model loop (MCP tools need something to call; the Development Engine needs the loop; Ornstein's quarantined summarizer IS a provider instance). The engine core is the critical path; the vision is sequenced, not skipped.

**Rationale:** the five research streams all point the same way — the integration surfaces (MCP, skills, tailnet) and amalgamations (oovra/GECK/sprint-loops) are additive layers over a CLI-first binary whose value is the policy-scaled, fully-traced local-model loop. Build the loop first, then layer.

## Artifacts

- `openclaw-integration.md` — gateway architecture, MCP-first verdict, security taxonomy lessons.
- `amalgamation-survey.md` — oovra (crate dep) / GECK (absorb) / sprint-loops (ferric-engine port) with sequencing.
- `deep-rust-feasibility.md` — Rust-CUDA/cuda-oxide/CubeCL verdicts; tree-sitter rustification verdict; non-Rust residue table.
- `docker-nix-tailscale.md` — bollard/gVisor container pattern, Nix-as-environment-compiler, Ornstein SOTA (dual-LLM + CaMeL-lite), Tailscale LocalAPI/SSH/serve verdicts.
- `mistralrs-integration-spec.md` — copy-paste-grade backend spec (crate pin, builder, constraint mapping, gotchas, HTTP fallback).

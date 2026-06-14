# Architectural Decisions

## ADR-001 — 2026-06-10 (sprint 0): Cargo workspace, harness-owns-decoding, dual-backend inference
Animus Ferric is a Cargo workspace of `ferric-*` crates. The harness owns decoding (constrained generation driven in the agent loop, not delegated to a server). Two Provider backends are planned: mistral.rs in-process (flagship pure-Rust path) and an OpenAI-compatible HTTP client (escape valve for Vulkan/AMD hardware and external servers). llama.cpp FFI bindings are permanently rejected: they import C++ UB into the agent process while buying coverage the HTTP valve already provides.

## ADR-002 — 2026-06-10 (sprint 0): JSONL trajectory is the source of truth
Every session writes a versioned JSONL trace (`v` field, per-session monotonic `seq`, flush per event). Readers MUST tolerate unknown event types (yield them with raw JSON preserved) so traces and binaries evolve independently. Tool results are traced FULL and untruncated; the model sees a truncated copy. All pretty renderings (CLI, future TUI) are derived views.

## ADR-003 — 2026-06-10 (sprint 0): Provider trait is async, dyn-compatible, constraint-carrying
`Provider` is `async` (via async-trait) and object-safe from day one because both s1 backends are async. `CompletionRequest` carries an llguidance-shaped `Constraint` (JsonSchema | Regex | Lark) even though s0's mock only records it. Streaming is a reserved extension point (`complete_stream` yielding `ProviderEvent`s), defined in s1. Backends own their heavy state internally; the trait is stateless per request.

## ADR-004 — 2026-06-10 (sprint 0): CPU-first portability, s0 dependency allowlist, aarch64 CI gate
The baseline target is CPU-only down to Raspberry Pi / Orange Pi class aarch64 Linux (and Jetson Orin); CUDA and AMD GPU paths are later, feature-gated specializations. s0 dependencies are limited to serde, serde_json, thiserror, async-trait (+ tempfile, futures-executor as dev-deps). No tokio/reqwest/ratatui/rusqlite until the sprint that needs them. CI gates `cargo check --workspace --target aarch64-unknown-linux-gnu` on every push.

## ADR-005 — 2026-06-10 (sprint 0): Security is hardcoded and harness-owned
Workspace containment is decided on canonicalized `Component` sequences (symlink-resolved, prefix-collision-proof — the lineage's CRITICAL `project-evil` bug is unrepresentable). Deny lists are compile-time constants with no runtime mutation API and no config override. The LLM is never consulted on a security decision.

## ADR-006 — 2026-06-10 (sprint 0): The scale function is pure, deterministic, and config-fed
`policy_for(&ModelProfile) -> RunPolicy` is a pure table lookup. Profiles are config-supplied — never inferred from filenames or GGUF metadata (the lineage's H8/H20 mis-detection traps). A measured L0–L6 capability level overrides the parameter-count prior in BOTH directions (downgrade an over-sized under-performer, upgrade an over-performing small model). Table values are a calibration seed pinned by a snapshot test; empirical calibration is a later sprint.

## ADR-007 — 2026-06-10 (sprint 0): Toolchain and lint gates
Edition 2024, stable toolchain pinned via `rust-toolchain.toml` (channel 1.93). `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` are merge gates on both Windows and Linux.

## ADR-008 — 2026-06-10 (sprint 0): All enumerated outputs are sorted
Tool lists, directory listings, and any other enumeration are deterministically ordered (registry uses BTreeMap; list_dir sorts). Reproducibility requirement inherited from Prion failure-mode #8 (map-iteration nondeterminism).

## ADR-009 — 2026-06-10 (sprint 0): MockProvider-only in s0; real-GGUF validation from s1
s0 ships only the deterministic scripted MockProvider. The lineage's validation policy is re-adopted verbatim from s1 onward: no change touching runtime, providers, grammar/constraints, or tier behavior merges without a real-GGUF run with tracing enabled. The first E2E gate is the s1 L0 smoke (one real-GGUF run produces a valid trace and a correct file edit), CPU-only and therefore hardware-independent.

## ADR-010 — 2026-06-11 (sprint 1): Constraint and native tool calling are mutually exclusive per request
A custom decoding constraint applies to the ENTIRE output and fights tool-call syntax. `CompletionRequest::validate()` rejects both-set requests; the loop validates before every provider call (primary), backends validate again at their boundary (defense in depth). The s1 loop uses native tool calling for tool turns and reserves `Constraint` for extraction turns; the harness-owned unified action grammar (one schema covering tool choice + task_complete) is s2 work. (Note: mistralrs 0.8.1 lacks the `strict` tool field present on master; strict argument grammars return when the dep is bumped.)

## ADR-011 — 2026-06-11 (sprint 1): Command structure has no chat catch-all
`ferric query` is the one-shot, workspace-scoped, policy-scaled, fully-traced surface for unstructured tasks; `ferric dev` is reserved for the Development Engine (the sprint-loops five-phase protocol ported as ferric-engine, s4–s7). No REPL/chat mode will exist. Additional surfaces (`ferric mcp`, `ferric serve`) are additive subcommands on the same CLI-first binary — Ferric is fully useful standalone.

## ADR-012 — 2026-06-11 (sprint 1): OpenClaw integration is MCP-stdio-first
Ferric exposes itself via `ferric mcp` (stdio JSON-RPC, s2–s3) plus a minimal auditable SKILL.md companion; the same surface serves Claude Code/Codex/Cursor, so the integration is not OpenClaw-specific. A WebSocket/HTTP peer is deferred until demand is proven. Per the OpenClaw security taxonomy (470 advisories): Ferric is the unified policy boundary for its own domain regardless of invoking layer — it never relies on a caller's exec filtering, treats every request as untrusted, and never echoes credentials into tool results.

## ADR-013 — 2026-06-11 (sprint 1): Ownership-graph boundaries are named, not absolute
The goal is a fully auditable ownership/lifetime/borrowing system; non-Rust residue must be explicit. Accepted named boundaries: tree-sitter's C core (re-examine per backlog — user-flagged lead 2026-06-11) and, on GPU paths, "Rust down to the driver ABI" (proprietary libcuda/NVRTC floor; CubeCL/Burn-LM is the all-Rust-kernel convergence target; NVlabs cuda-oxide on watch). CPU-only builds are ~100% Rust above the OS (pure-Rust gemm, tokenizers, GGUF parsing). No new non-Rust residue may enter without a new ADR. Follow-on: a committed, CI-verified ownership-graph attestation artifact (cargo-sbom/vet/deny class) so any build can be diffed against the repo's immutable record — the chain of trust over memory.

## ADR-014 — 2026-06-11 (sprint 1): Capability roadmap is pinned as backlog, not aspiration
Sequencing: s2 = oovra crate dep (per-tier prompt assembly) + unified action grammar + HTTP escape-valve backend (bounded reads) + circuit-breaker compaction + benchmark-harness port (L0–L6 calibration); s3 = GECK absorption (`ferric init-project`) + `ferric mcp` + Docker/Nix capability layer + Ornstein quarantine start; s4–s7 = ferric-engine (Development Engine); s3+ = tailnet surface (Tailscale LocalAPI whois + serve, SSH-reachable). Every entry lives in agent-tasks/ so it cannot evaporate. ADR-004 allowlist amendment (s1): mistralrs =0.8.1 (feature-gated, default off), tokio (cli, feature-gated), clap (cli, unconditional), futures-executor promoted to a cli regular dep; the aarch64 gate invariant is unchanged (default graph stays mistralrs/tokio-free).

## ADR-015 — 2026-06-13 (sprint 2): Unified action grammar via ActionProtocol
The action space is unified behind `ActionProtocol { NativeTools, UnifiedGrammar }` selected from `RunPolicy.protocol` × backend `Capabilities` × CLI override (`select_protocol`). `UnifiedGrammar` sends a constraint-only request (tools empty) carrying ONE llguidance JSON-Schema over every tool + `task_complete` as `anyOf` branches (`const` discriminator emitted first; never `oneOf` — llguidance 1.7.6 rejects it; `additionalProperties:false` at both depths). The whole completion is parsed as one `Action{tool,args}` and routed through the SAME dispatch path as native tool calls (terminator interception, repetition guard, permission events identical); results are framed as user-role `[tool_result for X]` messages. This makes the ADR-010 invalid state unrepresentable at construction. `finish_reason == "length"` is the one malformed-action case the grammar cannot prevent → `Completion.truncated` → nudge once → `StopReason::TruncatedAction` (distinct from parse-failure's `EmptyCompletion`).

## ADR-016 — 2026-06-13 (sprint 2): oovra dependency + s2 allowlist growth
ferric-prompt depends on oovra via git rev-pinned to `378abea` on main (lib API verified at that rev; immutable until a deliberate reviewed bump; crates.io at oovra v0.3). ADR-004 allowlist grows by: oovra (git), toml, regex, tempfile promoted to a ferric-bench regular dep, and `serde_json` gains `preserve_order` workspace-wide — LOAD-BEARING: llguidance emits JSON-Schema properties in document order, so the action grammar's `tool`-before-`args` early-branch-commitment requires insertion-order serialization (default BTreeMap would emit `args` first). All additions are pure Rust and aarch64-clean; the default graph stays mistralrs/tokio-free; ADR-008 sorted-output surfaces self-sort and are unaffected.

## ADR-017 — 2026-06-13 (sprint 2): HTTP escape-valve backend deferred s2 → s3
The OpenAI-compatible HTTP backend (llama-server/Ollama) moves from s2 to s3. Its integration spec is research-complete and banked (`sprints/s2/sprint-research/action-grammar-http-spec.md`), nothing in s2 depends on it, and s2's real-model validation budget is spent on grammar/prompt/calibration depth instead. s3 lands it together with bounded reads (Prion #3) and a shared `validated_complete` wrapper that makes ADR-010 backend-boundary enforcement model-free testable.

## ADR-018 — 2026-06-13 (sprint 2): Per-tier output-token budgets
`RunPolicy.max_output_tokens` is a per-tier snapshot-pinned seed (NANO 512 / SMALL 768 / MEDIUM 1024 / LARGE 1536 / XL 2048 / ULTRA 2048) driving `SamplingParams.max_tokens` in query and bench. Budgets leave headroom over the largest expected single action (~450-token write_file through L4) while capping a 1B's worst-case turn; they are recalibrated from bench `truncated_action` data, not folklore.

## ADR-019 — 2026-06-13 (sprint 2): Calibration pipeline (bench is the sole producer of measured_level)
`ferric bench` — embedded TOML L0–L6 specs, spawn-self release runner (`current_exe`, child always `query` so recursion is structurally impossible), trace-derived verification (completed = !timeout ∧ exit0 ∧ expectations ∧ tools ∧ clean terminator; `plan_steps` null — no planner yet, flagged not faked) — is the sole producer of `measured_level`. Results append-only to `results.jsonl`; `model_profiles.json` records measured_level + tier_from_params vs tier_from_measured, feeding `ModelProfile.measured_level` (ADR-006's bidirectional override). Tier assignments change only with a committed measurement diff.

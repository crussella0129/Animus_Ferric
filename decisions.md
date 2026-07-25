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
`ferric bench full` — embedded TOML L0–L6 specs, spawn-self release runner (`current_exe`, child always `query` so recursion is structurally impossible), trace-derived verification (completed = !timeout ∧ exit0 ∧ expectations ∧ tools ∧ clean terminator; `plan_steps` null — no planner yet, flagged not faked) — is the sole producer of `measured_level`. Results append-only to `results.jsonl`; `model_profiles.json` records measured_level + tier_from_params vs tier_from_measured, feeding `ModelProfile.measured_level` (ADR-006's bidirectional override). Tier assignments change only with a committed measurement diff.

## ADR-020 — 2026-06-15 (sprint 2): UnifiedGrammar is opt-in pending an engine-level hang root-cause
The s2 real-GGUF gate found that `mistralrs::send_chat_request` with a `Constraint::JsonSchema` attached HANGS on Llama-3.2-1B: the call never returns (the smoke trace stops exactly at `constraint_applied`, no `turn_end`), pegging ~20 cores for 4+ hours with zero output. Every model-free grammar test passes (MockProvider bypasses the real engine), so Ferric's loop/schema-generation/parse logic is correct — the pathology is downstream in llguidance grammar compilation or per-token masking over the generated schema on the GGUF tokenizer. This is exactly the false-green ADR-009 exists to catch. Decision (amends ADR-015's "default UnifiedGrammar for small models"): `select_protocol` auto-default is now `NativeTools` so `ferric query` never hangs by default; UnifiedGrammar is opt-in via `--protocol grammar`. The grammar machinery ships fully built and model-free-verified. Root-cause (minimal llguidance/mistralrs repro; suspect schema features — anyOf breadth, unbounded string args, or toktrie build cost; consider maxLength caps and a hard per-request inference timeout) is a tracked s3 task; the standalone `ferric query` path also gains a wall-clock kill so an engine hang can never run unbounded again.

## ADR-021 — 2026-06-23 (sprint 7): The PyO3/PyTorch backend is removed; external engines are reached only via the out-of-process HTTP valve
The embedded Python backend (added in s6 with NO governing ADR) loaded transformers/PyTorch inside the agent process via PyO3 and crashed with `STATUS_HEAP_CORRUPTION` — Rust's allocator and PyTorch's allocator sharing one process heap under repeated GIL acquire/release on a `device_map="auto"` offloaded model. It is deleted in full: `python.rs`, `inference.py`, the `backend-python` feature, and the `pyo3` dependency are gone (`cargo tree --all-features` shows zero pyo3). This closes the ADR-013 process gap — it had imported the largest possible non-Rust residue, a whole CPython+PyTorch runtime, into the agent's address space with no review — and restores the agent process to auditable pure Rust above the OS. The sanctioned way to reach any non-Rust engine (llama.cpp, Ollama, vLLM, even transformers) is the OpenAI-compatible HTTP valve (ADR-001): out-of-process, so foreign UB cannot corrupt the agent, and pure-Rust reqwest on Ferric's side. Gemma-4-e4b — the model the Python backend targeted — runs behind Ollama/llama-server via `--backend openai`. A network boundary to a separate server process is categorically different from an in-process FFI/PyO3 translational layer; it is exactly the boundary ADR-001/ADR-013 always intended. (User-confirmed direction, sprint 7.)

## ADR-022 — 2026-06-23 (sprint 7): Constraint reinstated, ADR-010 re-enforced, capabilities honest, action-protocol trichotomy
Sprints 3–6 silently abandoned the founding thesis. The `Constraint` was deleted from `CompletionRequest` and `validate()` became a no-op `Ok(())`; `ActionProtocol::UnifiedGrammar` sent NO constraint to any backend yet emitted a FALSE `ConstraintApplied` and regex-scraped `<tool_call>` XML — the "repair malformed output" approach the project exists to reject; and mistral.rs advertised `supports_native_tool_calls: true` while stripping tools from the engine (the s6 toolbench 0.0% fire rate). This ADR restores the thesis on the backend that can carry it and makes the code honest:
- `Constraint { JsonSchema | Regex | Lark }` returns to `CompletionRequest` (ADR-003); `validate()` re-enforces ADR-010 (constraint XOR tools — the invalid state is unrepresentable, rejected at construction and at the backend boundary).
- The OpenAI HTTP valve emits a server-enforced `response_format` JSON-Schema; this is the path where "the harness owns decoding" is actually true for 1B–14B GGUF, out-of-process, pure-Rust on our side (fulfils ADR-017's escape-valve + `validated_complete` intent).
- `Capabilities { supports_native_tool_calls, supports_constraint }` are a STRUCTURAL CONTRACT: a backend may only advertise what its `complete()` actually does on the path that runs. mistral.rs honestly reports neither — it strips tools and its grammar path hangs (ADR-020).
- `ActionProtocol` is an honest trichotomy `{ NativeTools, ConstrainedJson, TextXml }` (amends ADR-015's `UnifiedGrammar`; serde alias `unified_grammar` kept so older traces/bench rows still read). `select_protocol` reads capabilities: constraint→`ConstrainedJson`, native→`NativeTools`, else→`TextXml`. ONLY `ConstrainedJson` emits `ConstraintApplied`; `TextXml` is the honest unconstrained fallback (the model is prompted to emit XML and the loop scrapes it). mistral.rs's in-process *constrained* path stays backlog: the upstream llguidance/mistral.rs hang (ADR-020; the tokenizer.json fix was DISPROVEN), so constrained decoding lives on the HTTP valve, not the in-process engine.

## ADR-023 — 2026-06-23 (sprint 7→8): The HTTP valve is the workhorse; `ferric server` launcher (llama-server default); multimodal "any file" input; functionality outranks Rust purity
Standing principle (user, sprint 7→8): **optimal functionality outranks 100% Rust purity when they conflict** — this refines, not reverses, ADR-001/013's pure-Rust preference. The in-process pure-Rust mistral.rs path is therefore the *fallback*, not the workhorse; the out-of-process OpenAI HTTP valve (C++/Go server, pure-Rust reqwest client) is the primary, because its constrained decoding **and** multimodal actually work. (The in-process FFI/PyO3 translational layer remains rejected, ADR-021 — a network boundary is categorically different.) Decisions:
- **Launcher (sprint 8): a `ferric server` subcommand** owns the server lifecycle (the ADR-011 `ferric serve` slot): `up`/`down`/`status`/`doctor`, writes a runfile (pid+port+base_url) that `ferric query`/`ferric toolbench` auto-discover. **Default engine = llama.cpp `llama-server`** (libmtmd multimodal incl. audio+video; GBNF/json_schema constraints; pinnable binary+GGUF+mmproj for reproducibility, matching the ADR-008/013 attestation ethos), with **Ollama pluggable** via `--engine` (the OpenAI backend already talks to either; only the lifecycle manager differs). This is "the testbench for the new architecture" — `ferric server up` + the T-007 toolbench measures constrained fire-rate in one flow.
- **Multimodal "any file" input (sprint 8):** the one new harness capability. `Message` carries content *parts* (text OR media `{bytes/base64, mime}`); the OpenAI backend maps media to the OpenAI content array (`image_url` data-URL, `input_audio`), which llama-server accepts via libmtmd. Text/code files keep flowing as text via `read_file` (any model); media files (audio/video/image) route as multimodal parts, **capability-gated** — `ModelProfile`/`Capabilities` gain a `modalities` set; media is only routed to a model/backend that supports it, degrading gracefully otherwise (the ADR-022 honesty principle extended to modality). The target model "gemma-4-e4b" is **Gemma 3n E4B** (natively image/audio/video).
- **mistral.rs viability (sprint 8 research):** re-run the grammar_probe constrained-decoding hang test against the bumped **mistralrs 0.8.15** (the 0.8.1 hang of ADR-020 may be fixed upstream). Keep mistral.rs as the pure-Rust path iff its constraint now works; deprioritize/drop otherwise.

## ADR-024 — 2026-06-23 (sprint 8 test): The thesis is empirically validated; native tool-calling is unreliable through ollama; constrained is the default and the right one
The sprint-8 real-model E2E (full report: `sprints/s8/sprint-tests/findings-report.md`) ran the diagnostic toolbench against a live ollama (0.30.10) with `qwen2.5-coder:7b`. Findings, now load-bearing for future decisions:
- **The constrained-decoding thesis works on a real model.** Same model, same tools: the `ConstrainedJson` path (harness sends a JSON-Schema via `response_format`, the server enforces it) scored **100% (25/25, solid)**; the `native` path scored **0% (0/25)**. Ollama **does** honor `response_format` json_schema — the load-bearing E2E-1 unknown is resolved in favour of the thesis.
- **Native tool-calling is silently unreliable through ollama's OpenAI endpoint.** It returns the tool call as **text in `message.content`** (`{"name":…,"arguments":…}`) with `message.tool_calls` **null**, so the loop's native path sees no call (`no_action`). The diagnostic toolbench correctly *diagnosed* this (taxonomy: `no_action`), not merely counted it — vindicating the sprint-8 taxonomy design. The default protocol for the HTTP valve is already `ConstrainedJson` (via `select_protocol`, since the valve advertises `supports_constraint`), so the 0% native path is only reachable by explicit `--protocol native`. **No default change needed.** A native-fallback that scrapes a tool-call-shaped `content` when `tool_calls` is null is an optional low-priority robustness task.
- **The sprint-8 testbench is validated end-to-end** — `ferric server up`/`status`/`down`, runfile auto-discovery (toolbench with no `--api-base` found the launched server and scored 100%), and the diagnostic report all worked against real ollama. This unlocks **fleet calibration** (run the testbench across `D:\Models` 0.5B→14B to produce the real `measured_level` table, ADR-019) as a now-cheap, high-value option.

## ADR-025 — 2026-06-23 (sprint 9): Fleet calibration proves the constraint extends the floor to 1B; native fallback fixes ADR-024; mistral.rs 0.8.15 no longer hangs but still doesn't enforce
Sprint 9 cashed in the testbench. Full report: `sprints/s9/sprint-tests/test-report.md`. Decisions now load-bearing:
- **`ferric bench ltd --models <a,b,c>` (fleet sweep) is the calibration tool.** One command benches each model and emits a `render_leaderboard` table sorted best→worst (+ combined `.jsonl`, every row tagged by `model`). It is a **human-facing readout only — it does NOT write `measured_level`** (that stays `ferric bench full`'s job, ADR-019). Boundary preserved.
- **The constraint extends the usable model floor down to 1B — the thesis, demonstrated.** Real fleet run (qwen2.5-coder:7b, llama3.1:8b, llama3.2:1b; 5 tools × 10 iters): **ConstrainedJson = 100% solid on ALL three, including `llama3.2:1b`**, exactly where **NativeTools collapses to 22% unreliable** on the 1B. The 1B native failures are `malformed_args` (right tool, missing required args), not `no_action` — so the constraint wins precisely because it *forces* the `required` args the tiny model drops unaided.
- **T-902 native-`content` fallback fixes the ADR-024 gap.** OpenAI backend now recovers a tool call from `content` when `tool_calls` is null (ollama quirk). Native went **0% (s8) → 100%** on the 7–8B models, and the 1B's residual failures became a *diagnosable* `malformed_args` 22% instead of a silent zero. ConstrainedJson remains the default for the valve regardless; this only hardens the explicit-native path.
- **mistral.rs 0.8.15 viability — RESOLVED (the ADR-020/023 gate).** `grammar_probe` on Llama-3.2-1B: both `trivial` and `unified` (the exact schema that hung unboundedly in s2) **RETURN in ~10s — the ADR-020 hang is fixed upstream.** BUT both return the *identical* freeform JSON regardless of schema → **the `JsonSchema` constraint is not enforced** (zero effect on generation). **Verdict: mistral.rs stays the unconstrained TextXml fallback; it is NOT a constrained pure-Rust backend.** The constrained thesis stays on the HTTP valve (ADR-001/023), which the calibration just proved at 100% to 1B. Whether the non-enforcement is upstream or in our `MistralRsProvider` wiring is a future investigation, not a blocker. This closes the sprint-8 "mistral.rs viability" research item.
- **Sprint 10 candidate (unchanged):** multimodal "any file" input (ADR-023) — its E2E acceptance still needs a multimodal-capable server (ollama vision pull or `llama-server` install).

## ADR-026 — 2026-06-24 (sprint 10): Multimodal "any file" input pipeline shipped (input only); live-media E2E deferred to a heartbeat
Sprint 10 built the ADR-023 multimodal *input* path end to end, on the OpenAI valve, fully unit/integration-tested. Test report: `sprints/s10/sprint-tests/test-report.md`. Load-bearing decisions:
- **`Message` carries media additively.** A `media: Vec<MediaPart{mime, base64}>` field with `#[serde(default, skip_serializing_if="Vec::is_empty")]` — a media-free message serializes **byte-identically** to the pre-sprint schema (asserted), so every existing `msg.text` reader, trace, and test is untouched. This is the chosen alternative to replacing `text` with a content-parts list (far smaller blast radius).
- **Routing + gating is pure (`ferric-core::media`).** `classify_path(path)` (by extension) → `decide_attachment(kind, declared, supports_media)` → `{AppendText | Media | Skip(reason)}`. Text/code folds into the prompt (works on any model); media attaches **only** when its modality is declared (`--modality`, explicit config per **ADR-006** — never inferred) AND the backend can carry media (`Capabilities.supports_media`, honest per **ADR-022**: the valve = true, the in-process mistral.rs path = false). A skipped file prints its reason to stderr and is **non-fatal** — never a silent drop, never a hard error.
- **OpenAI content-parts mapping.** `map_message` emits the parts array (`text` + `image_url` data-URL / `input_audio`) only when media is present; text-only stays a plain string (unchanged wire shape).
- **Dependency-free base64 (ADR-004).** A ~15-line RFC-4648 encoder in-core (vector-tested) rather than a new crate, keeping the s0 dependency allowlist quiet.
- **The loop threads media** via `RunArgs.media` → `Message::user_with_media` for the first user turn; empty ⇒ identical to before.
- **Deferred (the sprint's one human-verification checkpoint):** the **live-media E2E** — a real multimodal model (Gemma 3n) actually reading an attached image/clip — needs a multimodal-capable server the dev machine lacks (route TBD: `llama-server`+mmproj, widest coverage, vs an ollama vision pull). Everything beneath it is tested; only "does the model read the bytes" is open. Rides the OpenAI valve only; mistral.rs stays text (ADR-025).

## ADR-027 — 2026-06-24 (sprint 11): mistral.rs 0.8.15 still HANGS on constrained GGUF decoding — the ADR-020 hang is NOT fixed; it stays text-only
A spike to settle the question ADR-025 left half-answered. Full report: `sprints/s11/sprint-tests/test-report.md`.
- **The setup that hid the truth:** `MistralRsProvider::complete()` had *deliberately stripped* the decoding constraint since the s3 pivot (the ADR-020 workaround). So the sprint-8/9 `grammar_probe` (ADR-025) measured the **stripped** path — mistralrs never received the constraint — and its freeform output was just an unconstrained run, NOT mistralrs "ignoring" a constraint. The real enforcement behaviour was untested.
- **The experiment:** sprint-11 T-1101 wired our `Constraint::{JsonSchema,Lark,Regex}` 1:1 to `mistralrs::Constraint::*` via `RequestBuilder::set_constraint` (the 0.8.15 API exists), then re-ran the bounded `grammar_probe`.
- **The result — definitive HANG.** With the constraint actually applied, `complete()` on Llama-3.2-1B hit the **5-minute engine timeout** (probe ran 314 s then panicked) on the **trivial** `{x:string}` schema — the simplest possible. The stripped path returns the same input in ~10 s. **So the ADR-020 llguidance/toktrie hang on GGUF is NOT fixed in 0.8.15.** A trivial schema hanging is conclusive; `unified` was not run.
- **Decision:** the wiring was **reverted** (strip restored, documented inline) — keeping it would 5-minute-hang `toolbench --backend mistral --protocol grammar` (a real regression). `Capabilities.supports_constraint` stays **false**; `select_protocol` keeps mistral.rs on **`TextXml`**. **The HTTP valve (ADR-001) remains the only constrained path** — proven to 1B in sprint 9. Revisit mistral.rs constraints only when upstream fixes llguidance-on-GGUF. This closes the ADR-020/025 thread for 0.8.15.

## ADR-028 — 2026-06-24 (sprints 13–14): Tool vocabularies are concentric rings; the active rings ARE the grammar; the cap trims from the outside in
The user's tool-design north star, now the architecture. Tool sets are **concentric rings** that widen as a model proves it can call them reliably; the loop builds the constrained action grammar from exactly the active rings, so **controlling the rings controls the grammar**.
- **Mechanism.** `ToolSpec` carries `ring: u8` (0 = the always-on navigate/mutate core). `ferric_core::ring_for_tier(tier) -> u8` (Nano→0, Small→1, Medium→2, Large/Xl/Ultra→3) is the capability→ring ceiling; because `tier` honours `measured_level` (ADR-019), a model is **promoted to a wider ring set by demonstrated reliability**, not size alone. `tools_for_policy` keeps tools with `ring ≤ ring_for_tier(tier)` and, when over `max_tools`, **trims from the outer ring first** (priority by `(ring, name)`), then returns the set name-sorted (ADR-008).
- **Bug it fixes.** The previous cap was an alphabetical `.take(max_tools)` after a `min_tier ≤ tier` filter — harmless at ≤6 tools, but once the 8th builtin landed it would silently drop whichever *core* tool sorted last (e.g. `write_file`) on a Nano model. Trim-from-outer makes the core inviolable.
- **Ring 0 (sprint 13, measured `solid` 100% to 1B):** `read_file, write_file, edit_file, list_dir, make_dir, delete_path`. **Ring 1:** `search_files, move_path` (find & organize). **Rings 2–3** reserved (planner/diff; MCP/external, ADR-012 — the latter still gated on the ADR-005 external-exec call). `RunPolicy` is unchanged (ring derives from `tier`), so the tier-table snapshot is untouched.
- **Follow-ons:** a `--max-ring` CLI override (decouple ring from tier for explicit "use exactly these rings"); wiring the toolbench's per-ring fire-rate directly into a measured ring promotion (the s13 100% is the `solid` bar). Both are additive; the core architecture is set here.
- **Amendment (sprint 15):** the `--max-ring` override **shipped**. `RunPolicy.max_ring: Option<u8>` (set by `ferric query`/`toolbench --max-ring`); `tools_for_policy`'s ceiling is `ring_for_tier(tier).min(max_ring.unwrap_or(MAX))` — **restrict-only** (an override above the tier ceiling is a no-op; expansion stays earned via `measured_level`). So `--max-ring 0` pins any model to the Ring-0 core grammar regardless of size. Proven end-to-end by a `--mock` test asserting the trace's `PromptAssembled.offered_tools`. The measured-promotion wiring remains the open follow-on.
- **Amendment (sprint 16):** ring **calibration** shipped. `toolbench --calibrate-rings` sweeps a model ring-by-ring (reusing `bench_model`/`verdict`, auto-stopping when a ring adds no tools) and reports the highest ring it drives `solid` — pure `recommend_max_ring`. Measured: qwen2.5-coder:7b AND llama3.2:1b both calibrate to `--max-ring 1` at 100%. This is the *measurement* side of the promotion; persistence follows in ADR-029.
- **Amendment (sprint 18):** **Ring 1 rounded out.** Added `find_files` (find by *name* — companion to `search_files`' content search) and `copy_file` (organize complement to `move_path`), making Ring 1 a coherent four-tool "find & organize" set (`search_files, find_files, move_path, copy_file`). Both pure-`std::fs`, guard-scoped, `ring: 1`. Small's `max_tools` (10) exactly fits Ring 0 (6) + Ring 1 (4) — no trimming; Nano still gets the 6 core. Re-bench: both models still calibrate `solid` through Ring 1 — widening the ring kept the grammar 100% reliable.
- **Amendment (sprint 19):** **Ring 2 seeded.** Added `multi_edit` (`ring: 2`) — an ordered, *atomic* batch of first-occurrence edits to one file (read-once / apply to a working copy / write-once-if-all-validate). It's the right Ring-2 tool for small models: more than the Ring-0 `edit_file`, yet still reliably emittable (a JSON array of `{old,new}` strings) unlike a line-numbered unified diff. Reachable once a model earns Medium (`ring_for_tier(Medium)=2`). The bench could not previously *reach* Ring 2 (its profile was hardcoded to Small) — added `toolbench --params-b <f32>` (default 8.0) so `--calibrate-rings --params-b 20` benches at the Medium ceiling and sweeps rings 0–2, measuring live whether a given model can drive the new ring.

## ADR-029 — 2026-06-25 (sprint 17): The model profile is a real input to `query` — durable promotion (read-back of `measured_level` + `calibrated_ring`)
`model_profiles.json` was **written but never read**: `ferric bench full` calibrated `measured_level` and `write_profile`'d it, but every `ModelProfile{}` (including `query`) hardcoded `measured_level: None`. The ADR-019 override was computed, stored, and ignored at run time. This closes the loop so a *proven* model automatically runs at its earned capability.
- **Persist.** `ModelProfileRecord` gains `calibrated_ring: Option<u8>` (additive `#[serde(default)]`). `toolbench --calibrate-rings` writes each model's recommended ring via `write_calibrated_ring` (a load-or-create merge that **preserves** any `measured_level`) to `--profile-dir` (default `benchmarks`).
- **Read back.** `ferric query --profile-dir` (default `benchmarks`) resolves (model name, protocol label) and, if a record exists, seeds `ModelProfile.measured_level` (→ tier via `policy_for`) **and** defaults `policy.max_ring` to `calibrated_ring`. `read_profile` is the sole consumer.
- **Composition.** `measured_level` *raises* the tier (capability earned); `calibrated_ring` *caps* `max_ring` at the proven ring (restrict-to-proven). A model promoted to a higher tier still only gets rings it demonstrated — earned, not assumed.
- **Safety.** Operator `--max-ring` still wins; a missing file / un-keyed model / `--mock`-without-`--model` ⇒ `read_profile` returns `None` ⇒ **byte-identical to an un-calibrated run**. Proven by a `--mock` CLI test (a written `calibrated_ring: 0` caps the trace's `offered_tools` to the core; no file leaves Ring 1 intact). The (model, protocol) key is derived the same way on both sides; a mismatch is a missed optimization, never a regression. `ferric bench full` remains the SOLE producer of `measured_level` (ADR-019 unchanged).

## ADR-030 — 2026-06-26 (sprint 20): The full agentic loop runs the constrained path on a real model (`bench --backend openai`); the L0–L6 ladder validated end-to-end
Every prior sprint measured **single-shot tool-call fire rate** (the toolbench). The product is a **multi-turn agent**, validated by the L0–L6 ladder (`ferric bench full`) — but its runner (`runner.rs:run_spec`) spawned the child `query` with only `--mock` or the **mistral GGUF** flags, and mistral constrained decoding **hangs** (ADR-027). So the full loop had **never** exercised the constrained path on a real model.
- **Wiring.** Additive `Invocation.openai: Option<OpenAiArgs>` + a pure `query_args(prompt, inv, ws)` (precedence openai → mistral → mock, unit-tested) + `bench --backend {mistral|openai}` (`--api-base`, `--model`). No new `query` surface — it already takes `--backend openai`. mistral/mock byte-identical.
- **Bug fixed (found by running it).** `task_complete` is a structured *terminator* (SessionEnd), not a dispatched ToolCall, so `parse_trace` never credited it and no spec's `expected_tools=["task_complete"]` could verify → every level falsely FAILED. `parse_trace` now credits the `task_complete` terminator as a called tool. (The loop's tracing was correct; the bench's accounting was not.)
- **Result.** `qwen2.5-coder:7b` (ollama, ConstrainedJson) **passes all L0–L6** — readonly tool → file rename → multi-step → construction → multi-file-with-test → mini-cli → full-todo-app (L6, 5 turns, 1110 tok) → **`measured_level 6`**, promoting Small→**Large** (the ADR-019 bidirectional override on real data). The sprint-17 read-back then auto-applies it (`query` prints `measured_level Some(6)`). The constrained multi-turn agentic loop is validated end-to-end, and the demonstrated-reliability promotion now runs on real bench data.
- **Amendment (sprint 21): fleet capability map.** `bench --models <a,b,c>` runs the full ladder per model (`run_levels` extracted; openai-only; a `measured_level` leaderboard). The fleet map: **qwen2.5-coder:7b → 6 (Large); llama3.1:8b → 5 (Medium); llama3.2:1b → none (fails L0).** Two findings: **(1) single-tool-call reliability ≠ agentic capability** — the 1B fires single tool calls at 100% (toolbench, all rings) yet can't *complete* even L0 multi-turn, so a 1B is a great constrained tool-caller but not an autonomous agent; **(2) specialization beats size** — the code-tuned 7B outranks the larger general 8B. The ladder still discriminates (6/5/none), so harder levels (L7+) are a nice-to-have for ranking *above* a 7B, not urgent. A low/absent level is a valid measurement (the fleet sweep exits SUCCESS).

## ADR-031 — 2026-06-26 (sprint 22): the 1B's multi-turn ceiling is a capability limit, not nudge wording
Following s21's finding (single-tool-call reliability ≠ agentic capability), diagnosed *why* `llama3.2:1b` fails L0 — from the kept trace, two failure modes:
- **repeat-not-terminate** (L0/L1): it emits a correct `list_dir`, gets the result, then calls `list_dir` *again* instead of `task_complete`, until the repetition guard stops it (`terminator: repetition_guard`, `tools_called: ['list_dir','list_dir']`).
- **semantic flailing** (L2): it calls `make_dir` 15× with *different* paths and never completes → `max_turns`. The repetition guard misses this (it matches identical action *signatures*, so same-tool/different-args isn't a repeat).

**Mitigation tried:** sharpened the first-repeat nudge from a soft conditional into a direct imperative naming the repeated tool ("you already called `<tool>` … call task_complete now"). **Result: no change** — the 1B re-emitted the same calls; L0–L6 still `measured_level: none`, identical failure modes. So the 1B's agentic ceiling is a **genuine capability limit** (planning / state-tracking / completion-recognition), **not** prompt wording.

**Decisions:** (1) the sharper nudge **ships anyway** — it's strictly better wording, helps mid-tier models that *do* read nudges, and can't regress capable models (they terminate before the first repeat). (2) The repetition guard's blind spot to **semantic flailing** (same tool, varying args, no progress) is a recorded future-hardening candidate (e.g. a no-progress / max-same-tool cap). (3) **The 1B's role is settled**: a reliable *constrained tool-caller* (100% single-shot, all rings) but not an autonomous multi-turn agent — Ferric's `measured_level`/tier machinery already encodes this (it stays Nano, gets the Ring-0 core grammar, and `measured_level` correctly refuses to promote it). No nudge or prompt fix changes that; agency needs a more capable model.

## ADR-032 — 2026-06-26 (sprint 23): llama.cpp (`llama-server`) is the first-class engine; ollama is a pluggable fallback
The launcher already *defaulted* to `llama-server` (`server.rs`; `command()` emits `llama-server -m <gguf> [--mmproj] -c <ctx> --host 127.0.0.1 --port <p>`), but every bench had only ever run against **ollama** — so the constrained valve on full llama.cpp was unproven. Validated it live (first time) and adopt it as the primary engine.
- **Why llama.cpp:** raw tok/s (no daemon wrapper); **context as wide as you want** (`-c <n>` / `-c 0` = full trained context — vs ollama's narrow `num_ctx` default), which directly helps agentic runs; the multimodal path (`--mmproj`); and **edge minimalism** — a single static binary + ggml DLLs, CPU/CUDA/Vulkan/Metal builds, runs on Jetson Orin Nano / Raspberry Pi (+ AI hat). ollama is a heavier Go daemon + registry.
- **Proven (live):** fetched the prebuilt `b9821` CPU/x64 release; pointed `-m` at an **ollama GGUF blob** (`~/.ollama/models/blobs/sha256-*` are raw GGUF — zero re-download); `ferric --backend openai --api-base :8080/v1 --protocol grammar` drove the constrained loop end-to-end (created a file) and a Ring-0 toolbench scored **36/36 = 100% solid — identical to ollama**. The OpenAI-compatible constrained valve (ADR-001) is engine-agnostic; nothing in the harness changed, and the loopback bind (ADR-005) holds.
- **Decisions:** (1) **llama-server is the recommended/documented engine**; ollama stays a one-flag fallback (`--engine ollama`). (2) The **ollama-blob reuse** is the documented way to run llama-server on models you already pulled. (3) `--ctx` (server `-c`) is the wide-context lever for agentic work; the policy's prompt-budget still derives from the *profile* ctx (`query --ctx`). (4) No launcher code change was needed — the contract was already correct + unit-tested; this sprint validated it and made it first-class in the docs.

## ADR-033 — 2026-06-26 (sprint 24): the multimodal pipeline is validated end-to-end (image → Ferric → llama-server `--mmproj` → a seeing model)
The multimodal *input* pipeline (`query --file --modality`, the `image_url`/base64 content-parts mapping) was built + unit-tested in sprint 10 (ADR-023/026) but **never run against a real vision model** — no multimodal server existed. Sprint 23 (llama.cpp first-class) unblocked it; this sprint ran it live.
- **Proven:** fetched **SmolVLM-500M-Instruct** GGUF + its mmproj (ggml-org) and served them with the prebuilt llama.cpp `b9821` (`llama-server -m … --mmproj … --port 8080`). A generated 96×96 **red square** went through `ferric query --file --modality image` → the server log shows `process_mtmd: encoding mtmd batch n_chunks=1` (Ferric's image reached the vision encoder), and a direct query in the *exact* `image_url` format Ferric emits returned **"Red."** — the model sees what Ferric sends. The base64/data-URL content-part mapping (`openai.rs:media_part_json`) is correct against real pixels.
- **Finding:** under the **constrained agentic grammar**, a sub-1B VLM (SmolVLM-500M) degrades free-form captioning (it echoed a system-prompt line into `task_complete` instead of describing the image). The *image still reaches the model*; the JSON grammar just confuses a very small VLM. Mitigation: use a larger VLM, or an unconstrained "describe" step (a future `--modality`-aware option to relax the constraint for a vision turn is a candidate). The pipeline is validated regardless.
- **Decisions:** (1) multimodal is **validated + supported** on the llama-server engine via `--mmproj`; the sprint-10 mapping needed no change. (2) No vision model ships with Ferric — users supply a VLM GGUF + mmproj (ollama's models are text-only, so no blob reuse for vision). (3) The constrained-grammar × tiny-VLM caveat is documented; relaxing the constraint per vision turn is a backlog item. ADR-023/026 unchanged. **(Superseded by ADR-035: the caveat is closed by a *capable* model, not a workaround.)**

## ADR-035 — 2026-06-27 (sprint 25): Gemma 4 E4B is Ferric's reference model — ~4B is the usable agentic floor; a capable model closes the multimodal caveat
The project's own data shows a **~4B agentic floor**: full-loop `measured_level` is llama3.2:1b → **none**, llama3.1:8b → 5, qwen2.5-coder:7b → 6; and sub-1B VLMs (SmolVLM-500M) garble under the agentic scaffolding (ADR-031/033). So the answer to "make small models usable" is **not** a `--chat` workaround for unusably-small models — it's a *capable small* model. The user pointed to **Gemma 4 E4B** (Google, June 2026): 4B effective, multimodal (vision + audio), function-calling, 128K context, edge-feasible.
- **Validated live** (official ungated `google/gemma-4-E4B-it-qat-q4_0-gguf`, served by the existing prebuilt `b9821` llama-server, `--mmproj`, no update needed):
  - **Agentic:** L0–L6 → **`measured_level 5`** (PASS L1/L3/L4/L5) — **matches the 8B, just below the 7B; vastly above the 1B (none)**. Confirms ~4B as the usable agentic floor. (L0/L2/L6 fails are mostly CPU-speed timeouts — L0 hit the 60 s cap; a GPU build would clear them.)
  - **Multimodal *inside* the constrained loop:** `task_complete("The image is a solid red rectangle.")` — a capable model describes the image **under the JSON grammar**, **closing the ADR-033 caveat with no harness change** (the `--chat` workaround is dropped).
  - **Constrained valve:** Ring-0 toolbench **100% solid**, like the rest of the fleet.
- **Decisions:** (1) **Gemma 4 E4B is the recommended reference model** — minimal-but-capable (4B), multimodal, function-calling, edge-feasible; the sweet spot Ferric targets. (2) **~4B is the practical agentic floor** for *these* models (stated as observed, not universal). (3) The multimodal-under-constraint caveat (ADR-033) is **closed by capability**, not a workaround. (4) Speed caveat: use a CUDA/GPU llama.cpp build for usable latency (CPU q4 timed out the simplest level). No Ferric code changed — this is a model + evidence decision.

## ADR-036 — 2026-06-27 (sprint 26): the audio modality is validated end-to-end — Ferric multimodal = vision + audio, both live on Gemma 4 E4B
Sprint 24/25 validated **vision**; Gemma 4 E4B also has a native **audio** encoder. Validated the audio path live (cached Gemma 4, a local Windows-TTS WAV — no download).
- **Grounded:** Ferric's `media_part_json` already maps an `audio/*` `MediaPart` → an OpenAI **`input_audio`** content block (`{data, format}`); llama.cpp added **Gemma 4 audio via a Conformer encoder** (PR #21421) and **llama-server accepts `input_audio`**; the prebuilt `b9821` logs `init_audio` when loading the Gemma 4 unified mmproj — audio is live, no update needed.
- **Proven:** a 16 kHz-mono TTS WAV of *"The quick brown fox jumps over the lazy dog."* → `ferric query --file speech.wav --modality audio --protocol grammar "transcribe … then task_complete"` → **`task_complete("The quick brown fox jumps over the lazy dog.")`** — an **exact transcription, inside the constrained agentic loop**.
- **Decisions:** (1) **Ferric multimodal is vision + audio**, both validated end-to-end on the reference model via llama-server `--mmproj`. (2) **No Ferric code change** — the s10 `input_audio` mapping was already correct. (3) Audio is "experimental" in llama.cpp (the server warns) and worked cleanly on clean speech; quality may vary by audio — the *pipeline* + the reference model's ASR are what's validated. (4) Together with ADR-035, this completes the Gemma-4-E4B-as-reference-model picture: ~4B, agentic (L5), **and fully multimodal (vision + audio)**, on a single edge-feasible llama.cpp binary.

## ADR-037 — 2026-06-27 (sprint 27): a no-progress guard closes ADR-031's second failure mode ("semantic flailing")
ADR-031 named two multi-turn failure modes; sprint 22 hardened the first (repeat-not-terminate — *identical* calls — via the repetition guard's sharper nudge). The **second was still unguarded: "semantic flailing"** — the model calls *the same tool with different args* every turn (`make_dir` ×15 with new paths) and never completes, grinding to `max_turns`. The repetition guard misses it **by design**: it canonicalizes the COMPLETE action signature (name **+ args**), so same-tool/different-args never registers as a repeat.
- **Mechanism:** a new `ProgressGuard` (`crates/ferric-loop/src/progress.rs`) mirroring `RepetitionGuard` but canonicalizing only the **sorted-unique tool NAMES** of a turn (arg-insensitive, order-independent via a `BTreeSet`). It tracks a consecutive same-name streak: **Warn** at `WARN_AT=4` (a course-correction nudge naming the repeated tool), **Stop** at `STOP_AT=5` → `StopReason::NoProgress` (trace reason `no_progress`; `Event::NoProgressGuard{action}`). Wired right after the repetition guard in `run.rs`.
- **Threshold rationale:** the tool is the sole operation for ~6 turns before the stop — comfortably above realistic same-tool runs yet well under every tier's `max_turns` (Nano 15 … Large 40), so it bounds wasted compute without encroaching on the repetition guard (which fires earlier, at 2 identical strikes). The guards **compose**: repetition catches identical calls; progress catches the different-args streak it lets through.
- **Decisions:** (1) **Honest scope (ADR-031):** this does **not** make a weak model complete a task — that is a capability limit, and nudging didn't move the 1B. Its value is (a) **bounding wasted compute** on any stuck model (fail fast — ~6 turns, not 15/40/80) and (b) a **precise diagnostic** (`no_progress` vs the ambiguous `max_turns`), which lets the bench distinguish flailing from ran-out-of-turns. (2) **False-positive tradeoff, accepted + documented:** the harness can't semantically tell "productive repetition" (write 6 files) from "flailing"; mitigations are the conservative threshold, the Warn course-correction, name-set granularity (any other tool resets the streak), and `max_turns` as the ultimate backstop. Tuning is a one-line const change if data argues for it. (3) **No bench change** — `verify.rs::completed()` already passes only on `None|task_complete|final_text`, so `no_progress` classifies as a non-completion automatically. The defining test asserts the guard catches exactly the ADR-031 gap: identical input where `RepetitionGuard`→`Proceed` but `ProgressGuard`→`Stop`.

## ADR-038 — 2026-06-27 (sprint 28): a repeated-failure guard completes the loop-hardening guard family
The repetition guard (ADR-031, identical signature) and the no-progress guard (ADR-037, same tool name) both key off the **actions** a model emits — neither keys off whether those actions **work**. A model can emit a *different* tool every turn that **all error** (wrong paths, denied permissions, malformed args) and never recover: the repetition guard resets (different signature), the no-progress guard resets (different name), so it grinds to `max_turns`. Added the third, **result-keyed** guard.
- **Mechanism:** `FailureGuard` (`crates/ferric-loop/src/failure.rs`); `observe_turn(dispatched, errored)` where a turn is a "failure turn" iff it dispatched ≥1 (non-terminator) tool and **every** one errored (any success = partial progress → reset; a zero-dispatch turn never trips it). **Warn** at `WARN_AT=2`, **Stop** at `STOP_AT=3` → `StopReason::RepeatedFailure` (trace `repeated_failure`; `Event::FailureGuard{action}`). Wired **after** the dispatch loop in `run.rs` (it needs the `is_error` results), gated on `terminate_with.is_none()` so a turn that ends in `task_complete` — even with an earlier failed call — is a success, not a failure-stop.
- **Threshold rationale:** tighter than the no-progress streak (STOP=3 vs 5) — a *failing* streak is a stronger stuck-signal than a *succeeding-but-non-advancing* one and rarely self-corrects past a nudge, so a faster stop saves more compute. The three guards **compose** by threshold: repetition (2 identical strikes) fires earliest; the failure guard (3 all-error turns) catches the different-tools-all-failing mode the other two reset on; no-progress (5) catches the slower succeeding flail.
- **Decisions:** (1) **Honest scope (ADR-031/037):** does **not** make a weak model succeed (capability limit); it bounds wasted compute on a model stuck failing and emits a precise `repeated_failure` diagnostic distinct from `max_turns`. (2) **False-positive tradeoff, accepted + documented:** a probe that 404s once or twice won't trip it (any successful call resets; a Warn precedes the Stop; `max_turns` backstops). (3) **No bench change** — `verify.rs::completed()` already treats a non-`task_complete`/`final_text` terminator as a non-completion. The defining test stops a model emitting **different** failing tools every turn while the repetition + no-progress guards stay silent — exactly their gap. The loop-hardening guard family (repetition / no-progress / repeated-failure) is now complete.

## ADR-039 — 2026-06-27 (sprint 29): `apply_patch` rounds out Ring 2
With the guard family done, pivot back to the **tool rings** (the north star). Ring 2 ("plan & apply structured changes") was seeded with `multi_edit` (s19, ADR-028) and proven drivable (qwen-7b calibrates `--max-ring 2` at 100%), but it was a single tool. Added the second Ring-2 tool the rings memory + backlog name as "the room to grow": **`apply_patch`** — apply a context-located unified diff to one file, atomically.
- **Not redundant with `multi_edit`:** (1) **context disambiguation** — `multi_edit` replaces the **first** occurrence of each `old_string` (`replacen(_,_,1)`) and *cannot* target the Nth; an `apply_patch` hunk carries surrounding **context** lines, so it locates a *specific* site. (2) **Diff-format familiarity** — models are heavily trained on git diffs. The defining test edits the **second** of two identical lines via context — provably impossible with `multi_edit`.
- **Mechanism:** `crates/ferric-tools/src/builtin/apply_patch.rs` (`ring: 2`, `PermissionLevel::Write`). Args `{path, patch}`; the patch is unified-diff hunks (`@@` headers, then ` `/`-`/`+ lines). **`@@` line numbers are ignored — hunks are matched by context.** Apply is **line-based** (split on `\n`, locate the first contiguous `context+removed` run, splice `context+added`, rejoin on `\n` — round-trips the trailing newline) and **atomic** (validate+apply all hunks to an in-memory working copy; write **once** only if every hunk locates — a failure leaves the file byte-identical, like `multi_edit`).
- **Decisions:** (1) **Single-file scope** (the `path` arg names the target); a multi-file patch (create/update/delete with cross-file all-or-nothing) is deferred as a clean follow-on. (2) **Ambiguous hunk** → first match in the current working copy (deterministic; more context narrows it — that's the feature). (3) Ring-gating: Medium now has **12** tools (Ring 0 `6` + Ring 1 `4` + `multi_edit` + `apply_patch`); Nano `6` / Small `10` unchanged (ring ceiling 0/1). Medium `max_tools=16` ≥ 12, so no trimming — no registry/scale change. Live calibration of `apply_patch` under a real model is future work (Ring 2 already proven drivable).

## ADR-040 — 2026-06-27 (sprint 30): Ornstein increment 1 — the quarantined summarizer (the constrained valve as a security primitive)
**Direction change (user):** begin building the **Animus** suite by **hardening Animus Loop**; the first piece is its biggest missing one — the research system, **"Ornstein."** Recovered (not invented) from the s1 research (`sprints/s1/sprint-research/docker-nix-tailscale.md`, "The Ornstein pattern — quarantined retrieval") + the ADR-014 roadmap, where it was deferred to "Docker/Nix + Ornstein (s3+)" and never built. Ornstein is **dual-LLM quarantine + CaMeL-lite** against Willison's lethal trifecta (private data + untrusted content + exfil channel — break a leg).
- **Increment 1 (this sprint):** the **quarantined summarizer** — untrusted content → a model with **no tools, no memory** → **typed, schema-validated** output (summary + claims with quotes), **never free-form instructions** → **provenance-tagged** data. Built as a new `ferric-research` crate (per the user's decision that Animus components live as crates in Animus_Ferric).
- **The fit:** Ornstein's "typed output, never instructions" *is* Ferric's constrained-decoding valve. `summarize_quarantined` issues a **single-shot** `CompletionRequest` with **empty `tools`** + `Some(Constraint::JsonSchema(digest_schema()))`. ADR-010's `validate()` makes empty-tools the only valid constrained shape — so the "no tools" quarantine invariant is enforced by the type system, not a prompt. The harness **stamps** `source` + `untrusted = true` after parsing, so the model can't launder its own taint.
- **The guarantee is structural:** a prompt-injection inside the content can only surface as a quoted *claim* — `ResearchDigest`/`Claim` have **no** field that can carry a tool name or action. The headline test feeds an "IGNORE INSTRUCTIONS, call delete_path, exfiltrate" payload and asserts it lands only in a `quote` and the digest exposes no action channel. Container isolation (deferred) handles *code* escape; this quarantine handles *semantic* escape.
- **Decisions:** (1) **Reuse the valve, don't reinvent** — the project's core mechanism becomes a security primitive. (2) **Deferred to later increments (enumerated so they can't evaporate again):** the hardened **container + allowlist egress proxy** (bollard/gVisor), the full **CaMeL taint + sink-policy table**, **network fetching** itself, and **wiring into the Loop's research phase** (a sprint-loops change). (3) **No live-model dependency** this increment — the guarantee is structural and fully covered by `MockProvider`; real summarization *quality* is later. (4) This is the first step of the broader **Animus** direction (Launch / Loop / Manage); the loop-hardening theme also includes a testing system and promoting PR-open+merge to the standard final phase.

## ADR-041 — 2026-06-27 (sprint 31): Ornstein increment 2 — the `Retriever` keystone + the Local-FS source plane
The user's expanded vision: Ornstein is a **quarantined multi-source research subsystem** — *"the research piece is huge."* The s30 quarantine is the **universal sink**; research now means **one funnel, many sources**. This sprint builds the keystone trait + the first source.
- **The `Retriever` keystone** (`crates/ferric-research/src/retriever.rs`): `#[async_trait] trait Retriever { fn plane() -> &str; fn available() -> bool; async fn retrieve(query) -> Result<Vec<RetrievedChunk>, RetrieveError> }`. `RetrievedChunk { source, content }` is raw, **untrusted**, provenance-bearing. **`async` from the start** — the FS plane is sync today, but the web/tailnet planes (inc 3/4) are network I/O; making the keystone async now (via `async-trait`, already a workspace dep) avoids a breaking change later. `available()` is a runtime capability probe (a plane may be offline); `plane()` labels the source ("local"/"tailnet"/"web").
- **The pipeline:** `research(retriever, provider, query)` runs the source → **quarantine** (`summarize_quarantined` each chunk) → `Vec<ResearchDigest>`. An *unavailable* plane is a no-op (`Ok(vec![])`), not an error — capability-probed multi-source research runs only the live planes.
- **The Local-FS plane** (`LocalFsRetriever`): walks a **confined `root`** (sorted ADR-008; skips `NOISE_DIRS`, binary/unreadable, and **symlinks** for escape-safety), matches files by **name or content** (case-insensitive), returns byte-capped chunks with `source` = relpath, `max_files`-capped. Reuses the `search_files` walk *pattern* but serves the Ornstein pipeline (whole candidate **documents** to the quarantine), not the tool registry.
- **Decisions:** (1) **Every source is untrusted, even local** — a local file can be a downloaded doc / cloned README / NAS share, so all content routes through the quarantine; the retriever adds defense-in-depth confinement (root + no-symlink-follow) on top. (2) **`LocalFsRetriever` is NOT `search_files`** — that's a model-callable tool returning match-lines to the planner; this is a programmatic research source returning documents to the quarantine, never to the planner. (3) **Build order (user-chosen):** Local FS (this) → **Tailnet/NAS FS** (inc 3, Tailscale LocalAPI enumerate + reach, substrate pre-scoped in s1) → **Web + hardened container + allowlist proxy** (inc 4; the trifecta's exfil leg, so its security layer lands last) → CaMeL taint/sink-policy + orchestrator + Loop wiring (inc 5). Fully deterministic (temp dir + `MockProvider`); no live-model/network dependency this increment.

## ADR-042 — 2026-06-28 (sprint 32): Ornstein increment 3 — the Tailnet/NAS-FS retriever (Tailscale SSH)
The second source plane behind the keystone: search a **remote** tailnet device's filesystem over SSH and feed matches to the same quarantine. `crates/ferric-research/src/retriever.rs` gains `TailnetFsRetriever`.
- **Transports:** `SshTransport { Tailscale, Plain{port} }` — `tailscale ssh <host> -- <cmd>` (keyless, identity-based; Linux tailnet devices) vs `ssh -p <port> -o BatchMode=yes -o ConnectTimeout=8 <host> -- <cmd>` (Termux-style sshd). Both observed in this fleet: switchblade (Linux) + the Pixel (Android/Termux).
- **The security core — remote command injection.** `ssh` runs its command through the *remote* shell, so the caller-supplied research **query** and remote **root** MUST be POSIX single-quote-escaped (`shell_single_quote`: `'…'`, embedded `'` → `'\''`) or untrusted research input becomes RCE on the remote host. `ssh_search_argv`/`ssh_cat_argv` build injection-safe argv (`grep -rIl -- 'Q' 'ROOT' | head -n N`; `cat -- 'PATH'`). This is the deliverable, **fully unit-tested** (`;rm`, `$(...)`, backticks). Defense in depth: retrieved content still flows through the quarantine, so a malicious *file* can't act either.
- **Pure core / live spawn split** (the `server.rs` precedent — `command()` tested, spawn not): the escaping + argv builders + `parse_status_devices` are pure + tested; `available()` (host online in `tailscale status`) + `retrieve()` (spawn search → cat per file → `host:relpath` chunks) are the live paths.
- **Decisions:** (1) **Live E2E deferred (user's call), documented.** Live probe this machine: `pixel-10-pro-xl` reachable (ping pong) but **no sshd** on :22 (Android has no Tailscale SSH server) or :8022 (Termux sshd not up); `switchblade` **offline**. So no SSH target was reachable; the deterministic core ships + is tested, and the live run (`research(&TailnetFsRetriever{…}, provider, query)` → quarantined `host:path` digests) is the recorded follow-up once a target's sshd is up. (2) **Same `Retriever` trait, same `research()`** — the tailnet plane plugs in with zero pipeline change; the funnel is source-agnostic. (3) Next: the **Web** plane + hardened container + allowlist proxy (inc 4 — the trifecta's exfil leg, security-heaviest, last), then CaMeL sink-policy + orchestrator + Loop wiring (inc 5).

## ADR-043 — 2026-06-28 (sprint 33): Ornstein — the research orchestrator (`research_all` across planes)
With two source planes built (local s31, tailnet s32), the multi-source payoff is querying **all available planes at once**. Added `research_all(retrievers: &[&dyn Retriever], provider, query) -> MultiResearch` to `crates/ferric-research/src/retriever.rs`.
- **Mechanism:** per retriever in order — probe `available()`; if available, `retrieve(query)` then quarantine (`summarize_quarantined`) each chunk whose `source` is **new** (a cross-plane `BTreeSet` dedup); push a `PlaneResult{plane, available, digests}`. Returns `MultiResearch{ digests (plane-ordered, deduped), planes (per-plane report) }`.
- **Decisions:** (1) **Dedup at the chunk `source` level, *before* the quarantine call** — a source surfaced by two planes (e.g. a NAS file reachable via both a local mount and tailnet) costs **one** model inference, not two; inference is the expensive resource. Proven structurally by a test: a shared source with a **one**-completion `MockProvider` script passes (a late dedup would exhaust the script). (2) **Per-plane outcome report** (`PlaneResult`) — the observability the eventual Loop research-phase wiring + the user need ("searched local + tailnet; tailnet offline; 3 digests"); deterministic by plane order (first plane to surface a source gets the credit). (3) **Unavailable plane = recorded no-op, never an error** — a capability-probed multi-source system runs only the live planes. (4) **`research()` (single-plane) untouched**; `research_all` is additive; the funnel/quarantine pipeline is unchanged. (5) The **Web plane (inc 4)** remains gated on a containerizer (re-probed absent on Windows + WSL this session); CaMeL taint/sink-policy + Loop research-phase wiring are the remaining inc-5 pieces.

## ADR-044 — 2026-06-28 (sprint 34): Ornstein — the CaMeL-lite sink-policy primitive
Designed jointly with the user. Flow control on top of the quarantine: a `ResearchDigest`'s text is untrusted, but nothing previously stopped that text — once echoed into a tool argument by the model — from reaching a side-effecting sink. Added `crates/ferric-research/src/sink.rs`: **taint tracking** (`TaintSet`, CaMeL-lite substring matching, no interpreter) + a **configurable sink policy** (`SinkPolicy::decide(permission, tainted) -> SinkDecision`) keyed off the existing `ferric_guard::PermissionLevel`.
- **Policy matrix:** untainted → always `Allow`. `Read` + tainted → `Allow` (reading isn't a dangerous sink; the workspace boundary already confines it). `Write`/`Execute` + tainted → the configured `SinkAction` (`Deny` | `RequireApproval` | `Warn`), mapped 1:1 to a `SinkDecision`.
- **Decisions:** (1) **All three enforcement modes ship, caller picks** (explicit user choice) — `Deny` for the autonomous default, `RequireApproval` for a human-gated deployment, `Warn` for an observability-first rollout — so the eventual wiring doesn't need a breaking change to switch modes. (2) **Pure primitive only this sprint** — no loop wiring. The enforcement point is deferred to the `registry.execute` chokepoint (`crates/ferric-tools/src/registry.rs`), beside the existing `check(permission, path)` call, once digests enter the agent's context via the research→loop wiring (a `sprint-loops` change). (3) **Conservative substring taint, accepted knowingly:** a benign value containing a tainted substring is flagged (false positives), which is the safe direction for a *write* sink; empty/whitespace tainted strings are dropped on insert so an empty digest field can't taint everything. (4) The end-to-end test (a tainted digest's injected quote, echoed into `write_file` args, is flagged and `Deny`d under the autonomous default) is the structural proof the policy will gate a real injected write once wired — mirroring the injection-containment proof from ADR-040.

## ADR-045 — 2026-06-29 (sprint 35): expert review + refactor — the full audit + four remediations
The first full-project audit sprint: what does Ferric need to become an operational, competitive,
safe product, staying efficient for edge/personal-compute deployment? Full findings (file:line
cited) are in `sprints/s35/sprint-research/research-report.md`. This ADR records the audit's
outcome: four small, immediately-effective fixes shipped, and an explicit, reasoned deferral list
for everything larger.
- **Audit method:** three background review agents (security/efficiency/product-completeness)
  were stopped by the user before completing; per instruction, not relaunched. The audit was
  instead done directly (file:line verified), cross-referenced against an external review (GLM-5-
  turbo) the user supplied — three factual corrections made there (see the research report):
  "over-engineered" reframed as safety-infra-ahead-of-functional-breadth; "no live-backend CI
  tests" reframed as the correct trade-off (CI can't depend on live GGUF models — the existing
  `#[ignore]`-gated + manual `bench --backend openai` pattern is the right answer); a crate-count
  slip and an unsupported "burnout" claim dropped.
- **Shipped this sprint:**
  1. **Read-side sensitive-file guard** (`ferric-guard`) — `PermissionLevel::Read` previously
     unconditionally `Allow`d; combined with the trace persisting full untruncated tool output
     (ADR-002), a workspace secret read by the agent landed in plaintext in the JSONL trace. Added
     `DENIED_READ_SEGMENTS` (credential stores minus `.git` — git-metadata reads are a legitimate
     agent need) and `DENIED_READ_FILES` (the write-file list plus `.env`, the most common
     real-world secret file, previously on no list at all). Write access to `.env` remains
     allowed — only reading an *existing* one is denied.
  2. **`ferric server` edge-tuning flags** — `command()` had no way to set CPU thread count, GPU
     layer offload, or batch size, the primary latency levers on Jetson/RPi-class hardware. Added
     `--threads`/`--gpu-layers`/`--batch-size` (llama-server only; accepted-but-ignored for
     Ollama, which doesn't take them as CLI flags).
  3. **`mistralrs` rev-pinned**, not floating on `branch = "master"` — matches the `oovra`
     reproducibility policy; a fresh `--features backend-mistralrs` build no longer risks silently
     picking up a different upstream commit.
  4. **`reqwest` switched to `rustls-tls`** — `cargo tree -e features` confirmed `default-tls`
     (native OpenSSL bindings on non-Windows) was active though unused in practice (Ferric only
     ever calls `http://127.0.0.1` per ADR-005 — TLS itself is dormant for current traffic). Pure
     dependency-weight/cross-compilation win for ARM edge targets, zero exercised behavior change.
- **Explicitly deferred, with reasons (not silently dropped):**
  - **CaMeL sink-policy wiring into `registry.execute`** — there is no live taint source yet
    (nothing today ingests a `ResearchDigest` into a running loop's context); wiring the check now
    would be dead plumbing that never fires. Deferred alongside the research→loop wiring itself.
  - **`ferric mcp` (ADR-012 activation) + a genuine raw chat mode (ADR-011 revision)** — already
    decided (2026-06-29, recorded in `agent-tasks/agent-tasks.md` + memory) but too large/security-
    sensitive for a refactor sprint; the chat mode specifically needs its own dedicated ADR on the
    security boundary. Natural next sprint(s).
  - **Shell/exec + git tools** — need a real permission-model extension (a sandboxed execute
    surface), not a quick add.
  - **Streaming, session resume, trace rotation** — each a real product gap, each its own
    focused increment.
- **Decisions:** (1) Fix small + immediately-effective now; defer large/premature/security-
  sensitive work explicitly rather than attempt everything in one sprint — matches the project's
  own proven pattern (Ornstein's five-increment build, the three-guard family built one at a
  time). (2) The panic-safety sub-audit (grepped `unwrap`/`expect`/`panic!` across the model-
  output, backend-response, and file-content surfaces) came back **clean** — worth recording as a
  positive finding, not just gaps. (3) Animus_Ferric's **GGUF-only** decision and the **ADR-011
  revision** (MCP + chat) were made earlier the same day, outside this audit's direct scope, and
  are recorded separately (`agent-tasks/agent-tasks.md`, memory) — not re-litigated here.

## ADR-046 — 2026-07-03 (sprint 36): `ferric mcp` — the ADR-005 security call, one tool, launch-time-fixed containment
The "ADR-005 security call" that blocked ADR-012 since sprint 1 is answered, and the MCP-stdio
server it unblocks is built. User-prioritized from the GLM-review "critical gaps" list; the
companion mistral.rs in-process-hang item was explicitly **dropped** (reprobed twice already,
ADR-020/027 — the HTTP valve remains the only backend that matters). Full research + plan in
`sprints/s36/`.
- **The security call — the exposed surface.** `ferric mcp` exposes **exactly one MCP tool**,
  `ferric_query` (`{prompt: string, files?: string[]}`), NOT Ferric's individual builtins
  (read/write/exec) and NOT the tool rings as MCP tool groups. Every `tools/call` runs one full
  constrained agent loop (`ferric-loop::run`) — the same one `ferric query` drives — so it inherits
  the workspace boundary, the `ferric-guard` permission checks, the tool-ring ceiling, the loop
  guards, and per-call JSONL tracing unchanged. MCP is a new **entrypoint**, not a new **decoding
  path** or a new **privilege**.
- **Structural containment (the key property).** Workspace root, backend, model, and protocol are
  `ferric mcp` **launch-time CLI flags** (`McpArgs`, mirroring `QueryArgs` minus `prompt`/`files`),
  fixed for the server-process lifetime — exactly as `ferric server` pins its engine to a closed
  enum and its host to loopback. The `ferric_query` tool schema has **no** `workspace`/`backend`/
  `model` field, so a compromised or confused MCP client *cannot* redirect containment or load a
  different model per call — the guarantee is unrepresentable in the wire protocol, not something a
  handler must remember to enforce. A dedicated test asserts the schema never grows those fields.
- **Hand-rolled, not `rmcp`.** The needed surface is deliberately narrow (one tool; no resources,
  prompts, sampling callbacks, or notifications), so the JSON-RPC 2.0 framing is hand-rolled in
  `crates/ferric-cli/src/mcp.rs` (~a few hundred lines of `serde_json`), reusing the `tokio`
  dependency Ferric already carries — zero new external protocol-implementation dependency, full
  auditability (ADR-013's ownership-graph goal). Revisit `rmcp` only if the surface must grow
  (resources/prompts, or the eventual Development Engine's multi-tool needs).
- **Launch-time-fixed profile is a deliberate divergence from `ferric query`.** The persisted
  profile (`measured_level`/`calibrated_ring`, ADR-029) is read ONCE at server launch and held for
  the process lifetime; a `ferric bench ltd --calibrate-rings` run against a *running* server is picked
  up only on restart. `ferric query` re-reads per invocation. Accepted, matching the same
  launch-time-fixed philosophy applied to workspace/backend/model.
- **Errors never crash the server.** A loop/provider failure on one `tools/call` returns
  `isError:true` (same convention as `ferric query`'s `StopReason::ProviderError` → exit-failure),
  and the serve loop keeps accepting subsequent calls — proven by an error-then-success-same-session
  test and a real-subprocess stdio E2E (`ferric mcp --mock`).
- **Divergence from the external "Production Ready Action Plan" (reviewed same day).** That doc
  slotted MCP into its Phase 2 (sprints 38–40) as a **separate `ferric-mcp` binary exposing tool
  rings as MCP tool groups**. We shipped it early (s36) as an **in-process subcommand exposing one
  `ferric_query` tool** — because exposing individual tools/rings over MCP is exactly the bypass of
  the agent loop + guards this ADR's security call exists to prevent. The doc's other future-task
  ideas (streaming via buffer-and-validate, session resume via JSONL replay, persistent config,
  shell/git tools, the dev engine, deployment hardening incl. the `oovra` supply-chain risk) are
  captured in `agent-tasks/agent-tasks.md` as reviewed backlog.
- **Still deferred (unchanged):** the raw **chat mode** (the ADR-011 revision's second half) — its
  own future sprint + own dedicated ADR on the chat security boundary; it is NOT touched here.

## ADR-047 — 2026-07-03 (sprint 37): streaming inference — `complete_streaming`, the `ConstrainedJsonScanner`, `ferric query --stream`
User-chosen sprint focus, framed as "a base architectural choice." Fills ADR-003's reserved
`complete_stream` extension point (never built — confirmed by grep, zero prior code) so `ferric
query` shows live text instead of a wall of silence during inference. Full research/plan/critique
in `sprints/s37/`.
- **The core design tension:** `ConstrainedJson` (the flagship path) returns every turn's
  completion — including the final `task_complete` answer — as ONE opaque JSON object. Raw token
  deltas of that aren't human-readable. Solved with a small incremental scanner
  (`ConstrainedJsonScanner`, `crates/ferric-provider/src/stream_scan.rs`) that recognizes exactly
  two signals in the accumulating text: the `"tool":"<name>"` field (a cheap early activity
  signal — reusing ADR-016's tool-before-args field-ordering discipline for a new purpose) and,
  only when the tool is `task_complete`, the live-decoded characters of `args.summary` — the one
  field that IS prose. Handles JSON string-escape sequences correctly across arbitrary chunk
  boundaries (including multi-character `\uXXXX` splits, holding back from the start of any
  incomplete escape). The false-positive-safety argument (no `args` string value can be misread as
  the tool key) rests on `action_schema` always emitting `tool` first (ADR-016) plus valid JSON
  syntax making a raw unescaped `"tool":"` decoy inside a string value structurally impossible.
- **`Provider::complete_streaming`** (`crates/ferric-provider/src/traits.rs`): a new trait method,
  `async fn complete_streaming(&self, request, on_delta: &(dyn Fn(StreamDelta) + Sync)) ->
  Result<Completion, ProviderError>`, with a **default implementation** that calls `complete()` and
  fires at most one `Text` delta with the full text — every provider that doesn't override this
  (`MockProvider`, `MistralRsProvider`) behaves identically to `complete()` with zero code, zero
  behavior change. Only `OpenAiProvider` overrides it with a real SSE-based implementation
  (`Response::chunk()` — no `stream` cargo feature or extra dependency needed; simpler than
  `bytes_stream()`, which would have required `futures_util::StreamExt`). Callback shape (not a
  `Stream`/`futures` return type) chosen deliberately: `Provider` must stay dyn-compatible
  (ADR-003), which an unboxed `impl Stream` return type breaks; the callback avoids a new
  dependency and keeps the return type identical to `complete()`.
- **The constrained-decoding guarantee is unchanged.** The constraint is enforced server-side by
  llama-server/Ollama regardless of streaming; buffering is for *display* only — the full JSON
  object is still parsed/validated/dispatched by the existing, untouched `ferric-loop` dispatch
  logic once the stream ends. `RunArgs` gained one field, `stream_sink: Option<&(dyn Fn(StreamDelta)
  + Sync)>` — `None` (every pre-sprint-37 caller) is byte-identical to today; `Some` routes the
  turn through `complete_streaming_with_backoff` (mirrors `complete_with_backoff`'s retry policy —
  a retryable mid-stream error retries the whole request fresh, so a failed attempt's deltas are
  never replayed or duplicated by the next attempt).
- **`ferric query --stream`** (opt-in this increment, not default-on): prints `Text` deltas to
  stdout live-flushed, `ToolNamed` as a stderr activity line ("▸ calling `<name>`..."); skips the
  final echo when streaming already displayed the answer (no duplication, proven for `--mock` where
  the default impl fires zero deltas for a tool-calls-only completion, so the existing final-echo
  path is the sole output — byte-identical to non-streaming).
- **Scope, deliberately bounded (explicit follow-ons):** `ferric mcp` streaming is OUT — its stdout
  is reserved exclusively for JSON-RPC frames (ADR-046); partial-text needs MCP's own notification
  mechanism, a separate future increment. mistral.rs backend streaming is OUT — it's the fallback
  path (ADR-023), not prioritized. Mid-stream retry beyond "restart the whole request" is OUT — no
  attempt to seamlessly resume/dedupe a partially-displayed stream. A structured JSON streaming mode
  for programmatic consumers (the reviewed production-readiness plan doc's second flag idea) is OUT
  — worth its own increment once raw human-readable streaming is proven.
- **Dependency note:** `reqwest`'s `stream` feature was considered, then dropped once
  `Response::chunk()` was confirmed to need no feature flag at all — a strict simplification, not
  an addition, to ADR-004's allowlist. `tokio` gained `net`/`macros`/`io-util`, dev-dependency-scoped
  only (a hand-rolled `tokio::net::TcpListener` fake-server E2E test proves the real wire protocol,
  mirroring sprint 36's real-process/real-socket testing preference) — NOT needed by
  `complete_streaming`'s production code path.

## ADR-048 — 2026-07-04 (sprint 38): persistent configuration + `Animus.md`
User-chosen (2026-07-04): "persistent config and Animus.md (much like claude.md but for Animus)" —
two related pieces, combined into one sprint since the user named them together. Full
research/plan/critique in `sprints/s38/`.
- **Config precedence:** `CLI flag > project (.ferric/config.toml) > user (cross-platform path) >
  today's hardcoded default`, resolved per field via `cli_arg.or(config.field).unwrap_or(default)` —
  extending `backend.rs`'s already-proven `resolve_base` idiom (ADR-008/T-805) one layer. `Config`
  (`crates/ferric-cli/src/config.rs`) is a **bounded, named field list**, never a generic key-value
  map: `backend`, `model_dir`, `model_file`, `model`, `api_base`, `api_key`, `params_b`, `quant`,
  `family`, `ctx`, `temperature`, `max_ring`, `profile_dir`, `stream` — this makes "config never
  touches security/guard/denylist policy" (ADR-005) a structural fact, not a review-time hope.
  `ferric server`/`ferric bench full`/`ferric bench ltd` are NOT config-defaulted this sprint (scoped to
  `ferric query`/`ferric mcp` only).
- **A field losing a clap default is a masking hazard, not a cosmetic detail.** Several
  `QueryArgs`/`McpArgs` fields (`params_b`, `quant`, `family`, `ctx`, `temperature`, `profile_dir`)
  used `default_value_t`/`default_value`, making "the user explicitly passed this flag" and "clap's
  baked-in default fired" indistinguishable — a config-only-set value would be silently invisible.
  Fixed by making all six bare `Option<T>`, with the real default applied AFTER merging config. A
  **plan-critic pass caught the same masking hazard twice more**: (1) `BackendOpts.backend` itself
  still carried a clap default (`"mistral"`) even though its 8 sibling fields didn't — fixed the same
  way (bare `Option<BackendArg>`, `.unwrap_or(BackendArg::Mistral)` at each of 4 call sites); (2) most
  significantly, `model_key` (the ADR-029 persisted-profile lookup key) was originally going to be
  derived from the RAW CLI `--model`/`--model-file` args rather than the post-merge, config-resolved
  values — meaning a config-only-set `model` would silently skip its profile lookup and lose an
  earned `measured_level`/`calibrated_ring` promotion with no error or trace. Fixed before it shipped
  (`cli::config_only_model_still_resolves_profile` is the regression test that would fail without the
  fix) — recorded here because it's a class of bug (a config value present in data but invisible to
  the code that decides behavior) worth watching for in any future config-surfaced field.
- **Malformed-layer diagnostics are testable data, not a bare `eprintln!`.** `load_layered_from`
  returns `LoadedConfig { config, diagnostics: Vec<String> }` — mirrors the existing `RunConfig::
  prompt_composition_error` pattern. `ferric query` (which has a trace sink) both `eprintln!`s and
  `Note`-traces each diagnostic; `ferric mcp` (no sink exists yet at launch time — each `tools/call`
  opens its own) only `eprintln!`s, matching the pre-existing treatment of `prompt_composition_error`
  at launch — a deliberate, considered asymmetry between the two surfaces, not an oversight.
- **The cross-platform user-config path is hand-rolled** (`user_config_path_from`, ~15 lines,
  Windows `APPDATA` → XDG `XDG_CONFIG_HOME` → `.config` `HOME`-fallback → `None`), not a `dirs`/
  `directories` dependency — matches ADR-004's minimal-dependency discipline. It takes an injected
  env-lookup closure rather than reading `std::env::var` directly, so every branch is independently
  unit-tested without mutating real process env.
- **`Animus.md` is trusted context, not Ornstein-quarantined content.** Unlike Ornstein's untrusted
  web/tailnet retrieval planes, `Animus.md` is authored by the workspace owner — the same trust tier
  as the codebase they're already having the agent operate on. Read as plain text (no parsing, no
  versioning) and appended as a distinct block to whichever system prompt already exists (oovra-
  composed, or `DEFAULT_SYSTEM_PROMPT`) — deliberately NOT forced into oovra's versioned element
  system, the wrong shape for unversioned freeform prose. Presence is traced as a `Note`; absence
  stays silent, matching the existing precedent that the ordinary default path (no `prompts_dir`
  configured) is likewise untraced.
- **ADR-010 is unaffected.** `CompletionRequest::validate()`/`select_protocol` operate on the final
  resolved values regardless of whether they originated from a CLI flag or a config file — the
  constraint/native-tools mutual exclusion has no config-awareness dependency to get wrong.
- **Explicit deferrals:** `ferric init-project` (a wizard to scaffold `config.toml`/`Animus.md` — v1
  only reads an existing, hand-authored file, same as `CLAUDE.md` needs no wizard); config-defaulting
  `ferric server`/`ferric bench full`/`ferric bench ltd`; an MCP-side `Animus.md`/config-diagnostic
  `Note`-equivalent once MCP grows a launch-time trace mechanism.

## ADR-049 — 2026-07-04 (sprint 39): session resume — `ferric query --resume <path>`
User-chosen (2026-07-04) from a shortlist (chat-mode ADR / MCP streaming notifications / session
resume). Full research/plan/critique in `sprints/s39/`.
- **Scope: resuming an INTERRUPTED, still-incomplete task only — not a chat-continuation
  mechanism.** Two follow-up questions during research materially narrowed this: (1) the user
  confirmed "resume an interrupted task" (process crashed/killed mid-loop; continue the SAME task
  with more turns, no new prompt needed) over "follow up on an already-completed task" (closer to a
  chat continuation, sitting nearer the line ADR-011 draws against a REPL/chat catch-all); (2) the
  backlog's `--save-interval` was reframed by the user, unprompted, into **context-budget
  compaction** — a real, independent gap (`RunPolicy.prompt_budget_tokens` is computed and traced
  but never enforced) big enough to be its own dedicated **sprint 40**, not bundled here. `replay`
  therefore refuses (`ReplayError::AlreadyStopped`) any trace that already reached ANY stop reason,
  clean or not — a session that finished isn't "interrupted."
- **Two new/extended trace events, both independently valuable beyond resume:**
  1. `Event::SessionPrompt { system, user, media }` — the original system+user prompt text was
     never recorded anywhere before (only derived metadata: `PromptComposed`'s lineage,
     `PromptAssembled`'s char counts). Written once per session, before `TurnStart(0)`, skipped only
     when resuming (no new initial prompt exists for that session).
  2. The terminator's (`task_complete`) `ToolCall` is now traced in EVERY protocol (still never
     dispatched/executed) — closes a real, pre-existing gap where a `NativeTools` session's summary
     was recorded nowhere in the trace at all (`ConstrainedJson`/`TextXml` already carried it in
     `TurnEnd.text`). `TurnEnd` also gained `truncated: bool` (was computed, never traced). Both
     additive (`#[serde(default)]`), both backward-compat tested against the old wire shapes.
  3. `SessionStart.resumed_from: Option<String>` links a resumed session back to the ORIGINAL
     session's id (not a file path — stable even if trace files move). A resumed session always
     starts a brand-new trace file (never rewrites/reuses the old one — sidesteps `JsonlSink::
     open`'s `next_seq`-always-starts-at-0 footgun and preserves ADR-002's "one immutable file per
     session" invariant); resume-of-a-resume chains are allowed and need no special handling
     (`replay()` only ever reads the ONE target file named by `--resume`).
- **A real design correction found only during implementation of `ferric-loop::replay`:** `TurnEnd`
  is written BEFORE dispatch in `run()`, not after — so "this turn has a `TurnEnd`" does NOT prove
  its tool calls/guard checks/results finished; a crash mid-dispatch leaves a `TurnEnd` on disk with
  an incomplete tail. The locked plan anticipated only "`TurnStart` with no matching `TurnEnd`" as
  the dangling case; the actually-correct, stricter signal is: a turn is committed only once a LATER
  `TurnStart` confirms its dispatch fully ran. This is a strict superset of the plan's literal
  wording (discards MORE cases, never fewer) — recorded here as an honest build-time refinement.
- **Guards start fresh on resume** (`RepetitionGuard`/`ProgressGuard`/`FailureGuard`'s internal
  turn-history is NOT replayed) — they exist to catch NEW flailing, not re-litigate old turns;
  replaying their exact state would add real complexity for no correctness benefit.
- **Protocol match is validated, tier/budget is not.** `--resume` rejects a trace whose recorded
  protocol doesn't match this invocation's resolved protocol (mixing message framings mid-
  conversation would corrupt context — a real correctness break). `PolicySelected`/`PromptComposed`
  are still written fresh on every run (recording the actual continuation's tier/budgets, which MAY
  differ from the original's if different flags are passed) — this is harmless, only affecting the
  forward budget ceiling, not correctness.
- **The system message is frozen from the replayed trace on resume** — `--prompts-dir`/`Animus.md`
  are silently inert for it (the assistant/user history already exists; there's no new initial
  prompt to recompose). `ferric query` surfaces this explicitly with a stderr note when `--resume`
  is combined with either, rather than leaving a user to wonder why an edited `Animus.md` didn't
  apply to a continuation — matching this project's established practice of naming silent-no-op
  risks (e.g. ADR-048's masking-hazard write-ups) rather than leaving them implicit.
- **One accepted, narrow approximation:** a `TextXml` turn's exact parse-error text isn't traced
  (only that a no-action nudge fired) — `replay` falls back to the generic protocol-keyed template.
  Explicitly tested (never panics, always produces a valid nudge), not silently glossed over.
- **A deliberate, small crack toward "follow-up" territory, named rather than hidden:** `--resume
  <path> "extra instruction"` appends the extra prompt as one more user message after the replayed
  history. Mechanically this is the SAME shape as the rejected "follow up on a completed task"
  use case — but it's confined to genuinely-still-incomplete traces (the `AlreadyStopped` gate is
  the real ADR-011 boundary, not the mere absence of an extra-prompt flag), so it stays within this
  sprint's locked scope while giving a resumed run one small, useful affordance.
- **Explicit deferrals:** context-budget compaction (sprint 40, its own dedicated sprint per the
  user's own call — see `agent-tasks/agent-tasks.md`); `ferric mcp --resume` (`McpServer`'s
  launch-time-fixed design, ADR-046, has no per-`tools/call` trace-file selection mechanism);
  `--save-interval` in any form (dropped from this sprint entirely, reframed into the sprint 40
  compaction work instead).

## ADR-050 — 2026-07-04 (sprint 40): context-budget compaction — `HistoryCompactor`
Carved out of sprint 39's research when the user reframed `--save-interval`, unprompted, into
context-budget compaction. `RunPolicy.prompt_budget_tokens` (70% of `ModelProfile.ctx`, tier-capped,
`crates/ferric-core/src/scale.rs`) was computed and traced (`Event::PolicySelected`) but never
enforced anywhere in `run.rs` — nothing stopped the ever-growing `messages: Vec<Message>` from
exceeding the model's real context window over a long session. Full research/plan/critique in
`sprints/s40/`.
- **Model-driven summarization, always-on, no CLI flag.** `HistoryCompactor` (new
  `crates/ferric-loop/src/compact.rs`) mirrors the repetition/no-progress/failure guards' precedent
  (ADR-037/038: unconditional construction at loop start) — safe because the trigger is a real,
  numeric threshold no existing (short) session/test can accidentally cross. Trigger: the last
  known `completion.input_tokens` reaches 85% of `policy.prompt_budget_tokens` (a `deepagents`-
  style precedent found in research; the user specified the mechanism, not an exact fraction).
  `KEEP_LAST_TURNS = 2` — the most recent 2 turns always survive verbatim (mirrors the Microsoft
  Agent Framework's `MinimumPreserved`/`keep_last_groups` floor, one of two external references
  reviewed during research). Both are fixed constants for v1, not per-tier `RunPolicy` fields —
  keeps the blast radius small (no `scale.rs`/tier-table changes); per-tier tuning is a clean future
  follow-on, not required for the mechanism to be proven.
- **Turn-number tracking is ABSOLUTE, not relative — a design correction made during the plan-critic
  pass.** An earlier draft planned a `turn_offset` accumulator to re-key turn numbers after each
  fold; the plan-critic found its role and update formula were never derived, only asserted (and
  separately found that `replay.rs`'s real `TurnStart{ .. }` match arm discards the turn number
  entirely — extending it required real new plumbing, not a drop-in addition). The shipped design
  removes the offset scheme entirely: both `HistoryCompactor` (`run.rs` side) and `replay()`'s
  reconstruction walk (`replay.rs` side) track `Vec<(u32 absolute_turn_number, usize
  start_index_in_messages)>` directly. This makes the fold-span boundary a closed form
  (`fold_to_idx = completed[fold_count].1` — "the start index of the first entry beyond the folded
  range," by construction of the slice split, no off-by-one derivation needed) and makes a resumed
  session's compactor (which may start counting from a nonzero `turns`) need zero special-casing —
  absolute numbers just work regardless of where counting starts.
- **The `replay()` extension is required, not optional — closing the sprint's single biggest risk.**
  Sprint 39's `replay()` had no concept of a mid-session history rewrite. Without extending it, a
  `--resume` of a compacted-then-killed session would resurrect the FULL pre-compaction history,
  silently defeating this sprint's entire purpose for exactly the long-running-session case that
  motivated it. `replay()` now tracks `committed_turn_starts: Vec<(u32, usize)>` (pushed inside
  `commit_and_reset!()`, using a new `PendingTurn.turn: u32` field captured from `TurnStart{turn}` —
  previously discarded) and, on `Event::HistoryCompacted{through_turn, summary, ..}`, partitions
  committed turns at `through_turn` (`partition_point`), truncates `messages` back to `head_len`,
  inserts the summary, and re-appends the preserved tail at shifted indices. Handles repeated
  compactions naturally: a second fold just re-partitions whatever survives from the first.
- **The ordering invariant is structural, not just conventional.** `HistoryCompactor::
  record_turn_start` for the CURRENT turn must run before `maybe_compact` — this is what lets the
  compactor's `completed` slice structurally exclude the in-flight turn (never just a code-review
  convention), and it's what lets `HistoryCompacted` always land in the trace AFTER
  `TurnStart(current)`, which is what lets `replay()`'s pre-existing "commit on next TurnStart" rule
  (ADR-049) safely finalize the previous turn before a fold reads it back out. A direct regression
  test (`history_compacted_traced_after_triggering_turn_start`) asserts the real trace byte-order,
  not just the downstream message-count effect (a plan-critic finding: "load-bearing" ordering
  claims deserve a direct test, not just correctness-by-convention).
- **Same-provider reuse — a documented architectural divergence.** Every framework surveyed in
  research (Microsoft Agent Framework, LangChain's autonomous context compression) assumes a
  cheaper, separate model dedicated to summarization. Ferric runs one local GGUF model per session —
  there is no second model to delegate to, so the summarizer reuses the SAME `provider` via the
  existing `complete_with_backoff` (accepting its retry policy's up to ~1.75s worst-case latency on
  a failed attempt as a cost, not a new one — reusing existing machinery beats inventing a second
  completion-calling convention).
- **Summarizer failure is non-fatal.** A provider error or empty summarizer output logs one
  `Event::Note` ("compaction skipped: ...") and leaves `messages` unchanged — compaction never
  aborts a session; matches the project's established surfaced-but-non-fatal convention (e.g.
  media-skip in `query.rs`).
- **Resume interaction: the entire replayed history is the compactor's protected head.**
  `ReplayedState` (sprint 39) exposes only a flat `Vec<Message>`, no turn-boundary metadata — a
  resumed run's `HistoryCompactor::new(head_len)` is constructed with `head_len` covering the FULL
  seeded history, so only NEW turns generated after resuming are foldable. This is a deliberate,
  documented v1 scope limit (the resumed prefix's size is already bounded at whatever it was when
  the original session last compacted or ended), not a correctness gap.
- **Explicit deferrals:** per-tier `COMPACT_TRIGGER_FRACTION`/`KEEP_LAST_TURNS` tuning (fixed
  constants suffice to prove the mechanism); chunked summarization for a pathologically large first
  fold (if a session runs long enough before ever triggering compaction, the foldable span itself
  could be large enough to strain the summarizer's own context — noted in research as a real risk,
  intentionally not solved this sprint); a hard truncation backstop for when even the summarizer's
  own output keeps the budget exceeded (would sit behind summarization as an emergency fallback, not
  replace it — matches the "Cure not Prevention" framing from research, with truncation reserved as
  the last resort).

## ADR-051 — 2026-07-09 (sprint 41): container architecture — sibling containers + a microVM airlock, not Docker-in-Docker
User-chosen focus was chat mode (the ADR-011-revision's unbuilt half), immediately reframed into a
platform-wide question: **should the whole Animus platform run containerized** (Docker preferred,
k8s/n8n considered — flexibility from one machine to a datacenter), with chat, Ornstein's search
subsystem, and possibly MCP each in **nested** containers, citing **Docker-in-Docker (DinD)** as the
mechanism. The user chose "container architecture only this sprint" (chat mode → sprint 42). This is
a **design + skeleton** decision; the concrete artifacts (`docker/Dockerfile`, `docker/docker-
compose.yml`) are live-validated (`docker build` / `docker compose config`) because Docker Desktop
was installed mid-sprint. Full research/plan/critique in `sprints/s41/`.
- **The DinD correction (the core finding).** Literal Docker-in-Docker — nesting a second `dockerd`,
  or mounting the host `docker.sock` — is a **security anti-pattern for isolating untrusted content
  specifically**: it's "operationally equivalent to full root on the host with extra steps" (a
  shared-kernel boundary; runc `CVE-2024-21626`, NVIDIA Container Toolkit `CVE-2025-23359`, kernel
  io_uring `CVE-2026-1109` each broke container→host in 2024–2026). **2026 practice for the
  untrusted-execution use case has moved to microVM-class isolation** — Docker's own **Docker
  Sandboxes** (GA Jan 2026, Windows/macOS native) gives real "build/run containers inside" via a
  hypervisor boundary with **no host-daemon access**; gVisor (userspace-kernel syscall interception)
  is the Linux-native alternative. The user's cited 2023 DockerCon DinD article addresses a
  DIFFERENT problem (CI that must build other images), not an airlock.
- **Two problems were named as one — split them.** (a) **Platform-wide deployment flexibility**
  (single machine → datacenter): an ordinary service-topology question, answered by **sibling
  containers via Docker Compose** today, Kubernetes-ready later. (b) **Security isolation for
  untrusted-content execution** (Ornstein's "airlock"): answered by a **microVM-class sandbox**, NOT
  nested Docker. Conflating them risks either over-isolating every service or under-isolating (b).
- **`ferric` + its inference backend stay co-located in ONE container (`ferric-core`).** `ferric
  server` pins the backend to **loopback only** (ADR-005, `crates/ferric-cli/src/server.rs` binds
  `127.0.0.1`, never a public interface). Splitting `ferric` and `llama-server` into separate
  containers would break that loopback guarantee without extra network-namespace plumbing (a real
  security-posture change). Co-location preserves today's guarantee completely unchanged. Ornstein
  and any future chat/MCP surfaces are **separate sibling services alongside** `ferric-core`, not
  nested inside it. The `ferric-core` build must use **`--features backend-openai`** — `ferric-cli`'s
  `default = []` would otherwise produce a binary that can't drive the co-located backend at all.
- **Skeleton scope-limits (deliberate, not oversights).** The Dockerfile targets **x86_64/Linux
  only** for now (the project's CI-gated aarch64 Pi/Jetson ambition — ADR-004 — is served later via
  buildx `--platform`; `docs/llama-cpp.md`'s install is asset-selection-by-hardware, so one arch per
  skeleton is honest); no `EXPOSE`/`ports:` publish path (loopback-only preserved structurally);
  `ornstein-search`/`chat` are compose STUBS, never presented as functional.
- **Explicit deferrals (each named, not dropped):** whether `ferric-research` (Ornstein) gets its
  own binary/service entrypoint to run as a truly separate container process, or stays in-process
  within `ferric-core` (answerable incrementally, matching Ornstein's own one-ADR-at-a-time
  sequencing, ADR-040–044); **MCP's own containerization** ("we can assess" — not forced before the
  architecture exists to decide it against); **chat mode's own build + its dedicated security-
  boundary ADR** (sprint 42, per the user's scope choice); **multi-arch images** (buildx
  `--platform`); actually **running** `ferric-core` end-to-end with a live model behind it (`compose
  up` + a real query — needs a mounted GGUF; the natural first step of whatever sprint operationalizes
  the container); a **CI `docker compose config` gate** (GitHub Actions has Docker — an available
  future hardening, disproportionate to add for skeleton artifacts now).
- **Blocker cleared mid-sprint.** The multi-sprint "no containerizer on this machine" blocker (had
  stalled Ornstein's web-retriever container increment since sprint 30, ADR-040) is **resolved** —
  the user installed Docker Desktop (`linux/x86_64` engine, WSL2 backend). Ornstein inc 4 is no
  longer install-blocked.

## ADR-052 — 2026-07-09 (sprint 42): raw chat mode — `ferric chat`, hybrid talk + escalate; the chat security boundary
This is the dedicated security-boundary ADR the **ADR-011 revision** (2026-06-29) required for the
second half of the `ferric mcp` + chat split. ADR-011 originally said "No REPL/chat mode will exist";
the revision approved building it (motivated by Animus IDE sending natural-language change requests
conversationally) but flagged that a genuinely conversational surface touches the "harness always
owns decoding" thesis and therefore needs its own explicit boundary. The user chose the **Hybrid
(talk + escalate)** shape. Full research/plan/critique in `sprints/s42/`.
- **`ferric chat` is a REPL** with launch-time-fixed containment (ADR-046 pattern): workspace,
  backend, model, and protocol are fixed CLI flags held for the session — no per-turn override.
- **Talk mode (the default) is the harness's FIRST unconstrained-completion path.** A chat turn with
  no leading command is a single `provider.complete()` with **empty `tools` and `constraint: None`**
  (the lawful ADR-010 "neither" case), and its output is treated as **text only** — printed and
  appended to the conversation history, **never parsed for tool calls, never dispatched, never
  touching the registry or `ferric-guard`**. The safety is **structural, not a prompt instruction**:
  the talk path simply never calls dispatch. This reverses ADR-011's *letter* (a chat REPL now
  exists) but not its *security spirit* (talk mode has no action channel and cannot change the
  workspace).
- **`/do <request>` escalates a turn into the EXISTING constrained agentic loop.** Escalation drives
  the unchanged `run_with_provider` → `ferric_loop::run()` + `ferric-guard` + the loop guards +
  JSONL tracing — exactly the `ferric query` path. No new decoding path or privilege on the action
  side; the conversation history seeds the run as a sprint-39 `ReplayedState` resume.
- **Escalation is USER-initiated, NEVER model-initiated (ADR-005).** The LLM is never consulted on a
  security decision; a talk completion can never promote itself into acting — only the human typing
  `/do` moves a turn onto the constrained action path. Talk output is appended as an assistant
  message and printed; it is never re-fed through the command parser, so there is no path by which a
  model's talk output could trigger an action.
- **`ferric-loop::run()` stays ALWAYS-constrained.** The new unconstrained talk path lives entirely
  in the CLI chat module (`crates/ferric-cli/src/chat.rs`) — explicitly separate and auditable at the
  CLI boundary, never inside the loop. "The loop always owns decoding" remains literally true.
- **Trace shape.** Talk turns are logged to a single chat-session trace file (a chat-level
  `SessionStart..SessionEnd` envelope with an `Event::Note` per talk turn + a `Note` per `/do`
  referencing its escalation file). Each `/do` opens its OWN fresh agentic trace file — mirroring
  `ferric mcp`, which opens a fresh file per `tools/call` precisely because `run()` emits a whole
  `SessionStart..SessionEnd` envelope every call and cannot be nested. (A plan-critic finding: the
  originally-planned "one held sink with embedded run() blocks" would have produced multiple envelope
  pairs in one file and broken `replay()`.)
- **`--mock` uses a fresh per-turn `MockProvider`** (talk-shaped vs agentic-shaped by turn kind) —
  a REPL's turn count is stdin-driven, so a single fixed script would exhaust unpredictably (another
  plan-critic finding). The session-held provider is real-backend-only.
- **Explicit deferrals:** a fancy TUI (plain stdin line-reading first); talk-mode streaming;
  a dedicated `Event::ChatTurn` trace variant (talk turns reuse `Note` for v1); wiring chat into the
  Animus IDE (a separate organ).

## ADR-053 — 2026-07-09 (sprint 43): Animus Launch — the GECK-successor bootstrapper (`ferric launch`), increment 1
Animus Launch is a named Animus-suite pillar (memory `animus-suite-direction`): "interview about
goals → scaffold a git repo with main+dev already established → hand off to the Loop", living as a
crate in the Ferric monorepo. This starts it. Research found GECK (`~/GECK`) is Python-only (the
memory's "partial Rust geck-cli" was stale) and does macro-prompt/memory scaffolding but **no git
bootstrapping** — so Launch's distinct value is the deterministic "interview → real git repo
(main+dev) + a sprint-loop-ready skeleton" flow. The user chose to build **both** the deterministic
scaffolder and the interactive interview in increment 1. Full research/plan/critique in
`sprints/s43/`.
- **Launch has a genuinely DIFFERENT security posture — the key architectural point.** It is
  **user-run, deterministic, and LLM-free**: it CREATES a new project workspace at an arbitrary
  path, so it is NOT a workspace-scoped agent operation. `ferric-guard`'s containment (which
  confines an *agent* to one workspace, ADR-005) does not apply and is deliberately not forced onto
  it. The **one real safety property is refuse-to-clobber**: `scaffold` proceeds only if the target
  `!exists()` OR (`is_dir()` AND is empty, counting hidden entries like `.git` as non-empty); a path
  that exists but is not a directory is refused. There is no LLM in the loop, so there is no agentic
  risk to contain — the risk is only "don't destroy the user's existing files," which the
  precondition covers.
- **Git is a named subprocess boundary (ADR-013).** `scaffold` shells out to `git` via
  `std::process::Command` as a closed set of subcommands (`init`, `add`, `commit`, `branch`) — never
  a shell, never an arbitrary command — the same auditability posture as `ferric server` → `llama-
  server`. Each git step runs to completion, its exit status is checked, and stderr is captured into
  a typed `LaunchError::Git` on failure (so a failed `commit` never silently proceeds). The initial
  commit uses a **fixed `-c user.name`/`-c user.email` identity** so the scaffold commit works even
  where git has no global identity (CI); the user makes their own-identity commits thereafter. The
  branch setup is `git init` → commit → `git branch -M main` (rename the default branch → `main`,
  portable across git versions) → `git branch dev`.
- **Placement:** a new `animus-launch` **library crate** holds the deterministic scaffolding logic
  (`LaunchSpec`, validators, `derive_initial_tasks`, `scaffold`), and a `ferric launch` **subcommand**
  drives it — mirroring `ferric-research` (Ornstein logic) ↔ `ferric-cli` (surface). Additive to the
  CLI-first one-binary design (ADR-011), exactly as `mcp`/`chat` were.
- **The interview is a hand-rolled plain-stdin wizard (no new dependency)** — matching the
  conservative allowlist (ADR-004) and the sprint-42 chat-REPL precedent. The answer→spec logic is a
  PURE function (`spec_from_answers`, unit-tested); prompts print to **stderr** (stdout stays the
  final report), only for fields not supplied by flags, in the fixed order name → path → goal.
- **Explicit deferrals (inc 2+):** the full GECK-style project-type *profile library* (inc 1 has a
  bare `--type` passthrough or none); the "begin work?" **Loop auto-hand-off** that actually launches
  a first sprint; environment detection; richer goal→task NLP; a fancy TUI.

## ADR-063 — 2026-07-17 (sprint 72): harness-internal observability via `tracing` (a second, orthogonal channel to the JSONL trace)
The codebase had grown to 12 crates and a multi-guard async loop (compaction,
hooks, background tasks, VCS snapshots, retries) with **zero structured
logging** — `tracing` was not even a dependency. The JSONL trajectory (ADR-002)
is the source of truth for *what the model did*, but it is the wrong tool for
*why the harness did what it did* (a hook failed, a retry fired, a guard tripped,
a tool took 4s): those are not model actions and do not belong in the replayable
trajectory. This ADR adds the diagnostic channel — deliberately **distinct from,
not folded into,** the trace.
- **Two channels, never conflated.** JSONL trace = LLM trajectory, flush-per-event
  to its sink file, replayable/diffable (unchanged). `tracing` = harness-internal
  diagnostics, to **stderr**, ephemeral, level-filtered. A given fact lives in one
  or the other by its nature (a `ToolCall` is trajectory → trace; "tool handler
  returned in 4ms" is diagnostics → tracing). Nothing was moved out of the trace.
- **Libraries emit; the binary subscribes.** `ferric-loop`, `ferric-tools`, and
  the `ferric-provider` openai valve link only the `tracing` facade and emit
  spans/events. **Only `ferric-cli`** links `tracing-subscriber` and installs the
  process-wide subscriber once, in `main()`, before any subcommand runs — so every
  surface (`query`/`mcp`/`chat`/`api`/`bench`/…) inherits one configured sink for
  free. Idiomatic, and it keeps the libraries subscriber-agnostic.
- **stderr, quiet by default.** Several surfaces treat **stdout as a machine
  channel** (`ferric mcp` speaks JSON-RPC there; `query`/`launch` write their
  report there). Diagnostics therefore go to **stderr**, and the default floor is
  **WARN**, so an ordinary run prints nothing extra. Verified end-to-end: at `-vv`,
  stdout stays byte-for-byte clean while stderr carries the span-scoped debug feed.
- **`-v` count + `FERRIC_LOG`/`RUST_LOG` override.** A global `-v/--verbose`
  clap flag (`-v` info, `-vv` debug, `-vvv` trace) sets the floor; a non-empty
  `FERRIC_LOG` (preferred) or `RUST_LOG` env filter overrides it entirely, so
  per-crate targeting (`FERRIC_LOG=ferric_loop=debug`) needs no rebuild. The
  decision logic is a pure, unit-tested function; a malformed filter falls back
  to WARN rather than aborting the run.
- **Always-on, not feature-gated.** With no subscriber the macros are near
  zero-cost, so gating them across crates would buy no measurable edge/ARM win
  for real maintenance cost. Exception: `ferric-provider` only instruments the
  openai valve, so its `tracing` dep rides the existing `backend-openai` feature —
  the default library build and the aarch64 check gate never compile it.
- **`ferric-guard` stays pure (a deliberate non-change).** Guard is a hardcoded,
  side-effect-free decision crate (ADR-005). Its `check`/`check_command` flow
  **only** through the `Registry::execute` chokepoint in production, which now
  logs every denial at WARN *with the tool-name context guard itself lacks*.
  Instrumenting guard directly would double-log with less context and give a
  security-critical primitive a logging side effect — so guard was left untouched
  and its `tracing` dep dropped.
- **Level discipline.** WARN is reserved for rare, genuinely-actionable events
  (guard-trip stops, provider errors after retries, each backoff retry, guard
  denials at the chokepoint). Per-turn conditions that recur in a common degraded
  setup (a VCS snapshot failing in a non-git workspace, once per turn) are **debug**,
  not warn, so quiet-by-default survives that case; the failure is still recorded
  as a trace `Note`. A buffer-writer capture test asserts a guard trip emits its
  WARN *and* that a clean run emits nothing at WARN.
- **Scope / deferrals (named, not dropped):** `ferric-research` (Ornstein),
  `ferric-vcs`, and `animus-launch` are not instrumented this increment — Ornstein
  already emits provenance via its digests, vcs/launch are short deterministic
  subprocess sequences; they slot into the same facade later with no design change.
  No JSON/OTel subscriber, no `#[instrument]` on tool handlers, no per-request
  body logging (only shape: model, message count, constrained-or-not).

## Documentation note (sprint 72)
`decisions.md` physically ended at ADR-053 (sprint 43) while the README timeline
cites ADR-055…062 (sprints 45–54): the full ADR prose for that span was kept in
per-sprint working memory (`sprints/`, gitignored) and summarized in the README,
but not appended here. This is a known ledger drift, recorded so a future sprint
can backfill 054–062 from the README bullets if the full prose is wanted. ADR-063
is numbered past the highest README-referenced ADR to stay globally unique.

## ADR-064 — 2026-07-17 (sprint 73): Agent delegation via ICM (Interpretable Context Methodology) — the filesystem IS the orchestrator
The backlog's "Agent Delegation Structure (ICM)" is answered by adopting
**Interpretable Context Methodology** (Van Clief & McDermott, 2026, provided by
the user) rather than a code-level multi-agent framework. The decision: for
Ferric's target — sequential, human-reviewed workflows on small local models —
orchestration belongs in **folder structure**, not in a coordination framework.
Numbered stage folders encode execution order; each stage's `CONTEXT.md` is a
contract; a five-layer context hierarchy scopes what each stage-agent sees. One
agent runs every stage; the folder structure IS the delegation logic. Full guide
in `docs/icm.md`.
- **Why ICM over CrewAI/LangChain/AutoGen.** Those frameworks solve a
  coordination problem that a *sequential, human-in-the-loop* pipeline does not
  have. ICM's control surface is plain files: reorder folders to change stage
  order, edit a markdown file to change a prompt, add/delete a folder to add/drop
  a stage, open the folder to inspect state. It also keeps each stage's context
  small and focused — the "lost in the middle" degradation of a monolithic
  40k-token prompt never occurs because the folder structure loads only the
  current stage's files. This matches Ferric's whole thesis (small models, small
  focused steps) far better than a general agent-team framework.
- **The five layers.** L0 identity (`Animus.md`/`CLAUDE.md`), L1 workspace
  routing (`CONTEXT.md`), L2 the stage contract (`stages/NN_*/CONTEXT.md`), L3
  reference material (`references/`, `_config/`) — stable across runs, the
  *factory*, internalized as constraints — and L4 working artifacts (a prior
  stage's `output/`) — per-run, the *product*, processed as input. Separating L3
  from L4 in the filesystem hands the model already-organized context.
- **Ferric-native security — the load-bearing adaptation.** ICM as published
  treats the workspace as trusted. Ferric does not weaken its guarantees: each
  stage is a **workspace-scoped run**, so `ferric-guard`'s containment (ADR-005)
  applies unchanged. `compose_stage` resolves EVERY contract-referenced path
  through `Workspace::resolve`, so a contract **cannot pull context from outside
  the workspace** — an `../../../../etc/passwd` input is refused at the boundary,
  proven by a test. Externally-*sourced* L4 content (a research stage fetching the
  web) routes through Ornstein's quarantine (ADR-040) — that composition is a
  later increment, named not silently assumed.
- **Placement mirrors `animus-launch`.** A new **`ferric-icm`** library crate
  holds the pure model (`parse_contract`, `IcmWorkspace::discover`,
  `compose_stage`, `plan`, `scaffold_workspace`); a `ferric icm` subcommand
  drives it. `ferric-icm` depends only on `ferric-guard` (for the boundary) and
  `thiserror` — deliberately light, no loop/provider deps, so it stays pure and
  fully unit-testable.
- **Increment 1 scope (this sprint) — the model, inspection, and scaffold; NOT
  live execution.** Following the project's increment discipline (Ornstein inc 1
  built the quarantine primitive before wiring; CaMeL inc 1 shipped the sink
  primitive with a "would-deny once wired" test; Launch inc 1 scaffolded before
  the loop hand-off), inc 1 delivers: contract parsing, workspace discovery
  (numeric — not lexical — stage ordering, so `10` sorts after `2`), guard-checked
  layered composition into an `OrchestrationPlan`, and an LLM-free
  refuse-to-clobber scaffolder. `ferric icm init` creates a runnable 3-stage
  skeleton whose contracts round-trip through the parser; `ferric icm plan` prints
  each stage's scoped context + provenance (which file at which layer, missing
  inputs flagged) with NO model in the loop — the delegation made inspectable.
- **Increment 2 (deferred, named).** `ferric icm run` feeds each composed stage
  prompt into `ferric-loop::run` (guard-contained, traced, constrained) in numeric
  order, with human review gates between stages (`--auto` to run straight through,
  each stage a fresh trace file like `ferric mcp`). The `OrchestrationPlan` inc 1
  produces is exactly its input. Also deferred: the GECK-style workspace-builder,
  Ornstein-quarantining of externally-sourced L4 content, and conditional/branch
  routing (ICM is sequential by design).
- **A missing input is data, not an error.** A declared L4 input from an
  upstream stage that has not run yet is recorded in provenance (`present: false`)
  rather than failing the plan, so a scaffolded-but-not-run workspace still plans
  cleanly. Scaffold `.gitkeep` placeholders are skipped when reading a working
  dir (a git placeholder is not an artifact).

## ADR-065 — 2026-07-17 (sprint 74): ICM increment 2 — live per-stage execution through the constrained loop
Increment 1 (ADR-064) built the ICM model and made the delegation *plan*
inspectable. This increment makes it *run*: `ferric icm run` executes each
stage's composed context through the same constrained agent loop `ferric query`
drives, in numeric order, with human review gates. Full guide in `docs/icm.md`.
- **Reuse the constrained path, don't reinvent it.** Each stage is driven by the
  existing `run_with_provider` (query.rs, `pub(crate)`) — the exact loop `ferric
  query`/`ferric mcp` use, so a stage inherits `ferric-guard`, the loop guards
  (repetition/no-progress/failure), context compaction, hooks, and per-session
  JSONL tracing for free. Config + provider are built once from the ICM root
  (mirroring `ferric mcp`'s launch) and reused across stages; only the workspace
  changes per stage. The one exception is the **mock**: it is a single-use
  scripted provider, so a FRESH mock is built per stage (each stage is its own
  agent session — the same reason sprint 42's chat REPL builds a per-turn mock).
  A real backend is stateless per request and its provider + the one tokio
  runtime are reused.
- **Per-stage containment is stronger than the paper — the load-bearing security
  decision.** Each stage executes bounded to its OWN `stages/NN_*/` folder
  (`Workspace::new(stage_dir)`), NOT the whole workspace. So a stage can only
  write inside its own directory — it cannot clobber a sibling stage's output or
  the shared `_config/`. This is possible precisely because inc 1's `compose_stage`
  already folds the prior stage's output into the context as Layer 4: a stage
  never needs cross-stage filesystem *reads*, so cross-stage data flows only
  through the composed context and the human review gate. The paper's model
  (one agent, whole-workspace access, writes-to-current-output by convention) is
  replaced by enforced containment — the Ferric way (ADR-005).
- **Halt on failure.** A stage's terminator is inspected: a successful stop
  (`task_complete`/`submit_plan`/final text) continues the pipeline; any other
  stop (max turns, provider error, guard trip) halts it, because a downstream
  stage reading an incomplete/untrustworthy output would compound the error. This
  needed the outcome, not just an exit code — hence `run_with_provider`'s
  `Result<LoopOutcome, String>` return (not the ExitCode-shaped `run_query`).
- **Review gates = ICM's "every output is an edit surface."** Without `--auto`,
  the run pauses on stderr after each stage (except the last in range); the human
  edits the output on disk, then continues (Enter) or stops (`q`). EOF/closed
  stdin proceeds, so a non-interactive run without `--auto` does not hang. `--from
  N`/`--to N` run a stage sub-range (re-run one stage after editing its input).
- **The composed prompt gained an Outputs directive (a small inc-1 refinement).**
  `compose_stage` now appends the contract's `## Outputs` as a "write your
  deliverables here" block, so a live agent knows where to write — data the plan
  had but the composed prompt didn't. Traces are harness-written (not
  agent-written), so they land centrally at `<workspace>/.ferric/trace/`, outside
  the per-stage boundary, one file per stage.
- **Deferred (unchanged from ADR-064):** Ornstein-quarantining of
  externally-sourced Layer 4 content, the GECK-style workspace-builder, and
  conditional/branch routing (ICM is sequential by design).

## ADR-066 — 2026-07-17 (sprint 75): Agentic cron — scheduled periodic agent tasks, bounded to Ferric's own operations
The backlog's "Agentic Cron Jobs" is delivered: a `.ferric/cron/` directory of
TOML job definitions and a `ferric cron` watcher that runs due jobs. The canonical
use case is `/dream every 12h` — periodically consolidating traces into memory.
Full guide in `docs/cron.md`.
- **A job runs a Ferric subcommand, NOT an arbitrary shell command — the security
  boundary.** A job's `command` is an *enum* (`dream`, or `query` with a prompt),
  not a free string. So cron can only ever trigger operations Ferric already
  contains (a `query` is a workspace-scoped, guard-checked agent run; `dream` reads
  traces and writes MEMORY.md). This is deliberately **narrower than the hooks
  system** (ADR, sprint 67), which runs arbitrary user scripts on loop boundaries:
  a hook is a per-workspace escape hatch the user writes; a cron job is a scheduled
  trigger, and an unbounded scheduled shell command is a materially larger standing
  surface. Bounding cron to Ferric's own verbs keeps the standing surface small
  and every scheduled action guard-contained. New commands extend the enum, not a
  shell.
- **Pure core, CLI driver — the `ferric-icm`/`animus-launch` pattern.** A new
  `ferric-cron` crate holds the pure, fully-unit-tested logic: parse a schedule
  (`30s`/`15m`/`12h`/`2d` + `hourly`/`daily`/`weekly`), parse+validate a job TOML,
  compute due-ness against an **injected** `now` (reads no clock itself), and
  read/write last-run state. The `ferric cron` CLI drives it and performs
  execution by shelling out to the same `ferric` binary (`current_exe()`), running
  each job in the workspace — the same self-invocation pattern used elsewhere.
  `ferric-cron` depends only on serde/toml/thiserror.
- **Interval schedules, not full cron expressions (a deliberate scope choice).**
  The use case is "every N hours", so schedules are simple recurrence intervals,
  not five-field crontab syntax (no "every weekday at 09:00"). Interval + a last-run
  timestamp is enough, far simpler to reason about, and covers `/dream every 12h`.
  Calendar-anchored cron expressions are a named later extension.
- **State advances on ATTEMPT, not success.** After a due job runs (whatever its
  exit), its `last_run` is set to now, so a persistently-failing job reschedules to
  its next interval instead of firing every single tick. State
  (`.ferric/cron/.state.json`) is a runtime cache kept OUT of the user-authored job
  files, and a missing/corrupt state file degrades to empty (never a hard failure).
- **Surfaces:** `cron add` (scaffold a job file, refuse to overwrite),
  `cron list` (schedule + last-run/next-due), `cron run [--dry-run]` (one tick —
  run due jobs, or just report them; the tick an external scheduler could also
  drive), and `cron watch [--interval]` (the loop — a foreground daemon that ticks
  and runs due jobs until Ctrl-C, reusing the sprint-65 graceful-interrupt
  `tokio::signal::ctrl_c` pattern). A `query` job may set `mock = true` to run
  offline — useful for testing a schedule without a live model, and the hook that
  makes `cron run` fully E2E-testable.
- **Deferred (named):** calendar/crontab expressions; a detached watcher daemon
  with a runfile + lifecycle management (`ferric cron watch` is foreground for v1,
  backgroundable by the shell or the sprint-68 task machinery); catch-up/misfire
  policy for a watcher that was down across a due window; more job command kinds
  (e.g. an ICM pipeline run) as the enum grows.

## ADR-067 — 2026-07-17 (sprint 76): calendar cron expressions for agentic cron
Extends sprint 75's `ferric-cron` (ADR-066): a job's `schedule` now accepts a
standard 5-field **cron expression** (`0 2 * * *`, `0 9 * * 1-5`, `*/15 * * * *`)
alongside the existing recurrence intervals, so calendar-anchored tasks ("every
day at 02:00", "weekdays at 09:00") are expressible, not just "every N hours".
- **`Schedule` becomes an enum** — `Interval(ms)` or `Cron(CronExpr)`.
  `parse_schedule` dispatches on shape: a five-whitespace-field string is a cron
  expression, anything else an interval. The `cron watch` **tick** interval stays
  interval-only (`parse_interval_ms`) — it is "how often to check", never a
  calendar rule.
- **Evaluated in UTC — a deliberate correctness-for-testability trade.** Cron
  matching decomposes `now_ms` into UTC civil time and matches the fields, so
  due-ness stays a **pure, deterministic function of (expression, epoch-ms)** with
  no dependence on the host timezone. That keeps the whole scheduler unit-testable
  with an injected `now` (the sprint-75 property) and avoids flaky,
  environment-dependent tests. Local-timezone expressions — which would make the
  function OS-dependent — are a named deferral.
- **Fire-once-per-minute semantics.** A cron job is due when the current UTC
  minute matches AND it has not already fired within that minute (`last_run <
  minute_floor(now)`). With the default 60s watch tick, each matching minute fires
  exactly once; a job that runs at 02:00:05 will not re-fire at 02:00:55.
- **Standard field grammar + the Vixie day rule.** Each field supports `*`, a
  number, a range (`1-5`), a list (`1,3,5`), and a step (`*/15`, `0-30/10`), bounds-
  checked per field; day-of-week is `0-6` with `7` also Sunday. When BOTH
  day-of-month and day-of-week are restricted the job fires when EITHER matches
  (the de-facto Vixie-cron behavior), else both must match — matched by a test.
- **`chrono` added to the allowlist (ADR-004) at zero new cost.** It is used only
  for the epoch→UTC-civil decomposition; it was already in the dependency tree via
  `oovra` → `ferric-prompt` → `ferric-cli` (and already CI-gated on aarch64), so
  adding it as a direct dep of `ferric-cron` compiles nothing new. `next_due_ms`
  for a cron schedule is a bounded forward minute-scan (~366 days) for the `list`
  display; a job that never matches within a year reports no next-due rather than
  scanning unboundedly.
- **Deferred:** local-timezone cron; a detached watcher daemon + runfile;
  misfire/catch-up for a watcher down across a due window.

## ADR-068 — 2026-07-17 (sprint 77): `.ferricignore` — user-authored, additive-only path denials
The backlog's "Dynamic Denylist Configuration" is delivered as `.ferricignore`: a
gitignore-flavored file in the workspace root listing paths the agent must not
touch (`secrets/`, `*.pem`, vendored trees). This is the FIRST config-driven input
to security policy, so it is scoped tightly against ADR-005.
- **Additive-only — the load-bearing invariant.** ADR-005 makes security hardcoded
  with "no config override". `.ferricignore` does not override it: a pattern can
  only ADD a denial, never turn a hardcoded `Deny` into an `Allow`.
  `check_with_ignore` evaluates the compile-time floor FIRST and short-circuits on
  a hardcoded `Deny`; only a path the floor already ALLOWS is then tested against
  the ignore patterns. So the file strictly narrows what the agent may reach — the
  exact property that keeps it consistent with ADR-005's spirit (the hardcoded
  minimum is immutable; the LLM is never consulted) while relaxing only its letter
  ("no config override" → "config can only further restrict"). This is distinct
  from ADR-048's general config, which still may NOT touch security/denylist policy
  — `.ferricignore` is a dedicated, security-specific, additive channel.
- **User-authored, model-immutable.** Like `Animus.md`/`.ferric/config.toml`, the
  file is written by the human, never the LLM. To stop the agent disabling its own
  restrictions, `.ferricignore` is added to `DENIED_WRITE_FILES` — the model cannot
  edit or delete the policy that constrains it (symmetric with the `.ferric` trace
  protection).
- **Ignored ⇒ off-limits at every level.** A matched path is denied for Read,
  Write, AND Execute — the intent is "keep the agent away from this entirely", not
  a per-permission rule. Denials surface with `rule: "ferricignore"` and the
  matched source line, traced like any guard decision.
- **Placement + wiring.** A pure `IgnoreList` (`ferric-guard/src/ignore.rs`) parses
  the file (blank/`#` lines skipped) into three matcher kinds — a bare **segment**
  (`secrets`, matches that component anywhere), a basename **glob** (`*.pem`, simple
  `*`-only, no new dependency), and a **path prefix** (`data/private`, anchored at
  root). `Workspace::new` loads `<root>/.ferricignore` once (absent → empty no-op);
  the registry chokepoint calls `check_with_ignore(perm, resolved, root, ws.ignore())`
  — the single security decision point stays single. Matching is case-sensitive
  (POSIX/gitignore convention).
- **Scope / deferrals:** no negation (`!pattern`) un-ignore syntax (would risk the
  additive-only invariant and is unnecessary — the file only ever adds); no `?`/`[]`
  glob metacharacters (only `*`); `.gitignore` is NOT auto-consumed (an explicit,
  security-reviewed file, not an incidental one).

## ADR-069 — 2026-07-17 (sprint 78): direct terminal passthrough in chat (`!cmd` / `/run cmd`)
`ferric chat` gains a third turn kind alongside talk and `/do` escalate: a line
prefixed `!` (or `/run `) runs a shell command **directly, with no LLM
roundtrip**. Motivated by the interactive workflow — checking `ls`/`git status`
mid-conversation without asking the model to do it.
- **Human-initiated, guard-screened, no LLM — the security shape.** The command
  runs through the SAME `shell_exec` registry chokepoint the agent uses, so
  `ferric-guard::check_command` (the ADR-005 command denylist) still fires — a
  test drives `!rm -rf /` and asserts it is blocked. Crucially, there is **no LLM
  in this path**: the human typed the command, so ADR-005 ("the LLM is never
  consulted on a security decision") is satisfied trivially — the model neither
  proposes nor sees it. This slots cleanly into the chat security model: talk (no
  action channel), `/do` (LLM-driven constrained action), `!cmd` (human-driven
  direct action). The one that touches the LLM (`/do`) stays fully constrained;
  the one that acts without constraint (`!cmd`) has no LLM.
- **Not folded into the conversation.** A `!cmd` line and its output are a
  terminal side-channel, printed and logged as a chat `Note`, but NOT appended to
  the talk history — running a command is not saying something to the model. (A
  future increment could optionally surface output into context on request.)
- **Parsing is exact.** `!<cmd>` and `/run <cmd>` map to `ChatInput::Run`; a bare
  `!` or `/run` (no command) is *talked*, not run, and `/running late` is talked
  (the `/run ` boundary is required) — so the passthrough never silently
  swallows ordinary text. Pure and unit-tested, matching the existing
  `parse_chat_input` discipline.
- **Runtime wiring.** `shell_exec` runs on tokio (`block_in_place`, sprint 67),
  but the chat REPL is synchronous with no ambient runtime — so the passthrough
  lazily creates a multi-thread `tokio::runtime::Runtime` on first `!cmd` and
  reuses it. (The escalate path already had a runtime via the backend; talk-only
  and passthrough-free sessions never pay for one.)
- **Deferred:** streaming a long command's output live (currently printed after
  it completes); a way to fold command output into the model's context on demand;
  `!` passthrough in the non-REPL surfaces (it is chat-only by design).

## ADR-070 — 2026-07-19 (sprint 79): interactive "accept edits" mode (`--accept-edits`)
`ferric query --accept-edits` pauses before every **mutating** tool call, previews
it, and lets a human approve or reject it before it touches disk. Motivated by the
supervised-run workflow — watching a small model work and vetoing a bad write
without aborting the whole session.
- **Gated at the single dispatch chokepoint, keyed on permission — not tool name.**
  The loop already routes every tool call through one dispatch point in `step()`;
  the gate lives there, immediately after the `ToolCall` event is traced and before
  `registry.dispatch()`. Whether a call is "mutating" is asked of the guard's own
  permission model via the new `Registry::permission_of(name)` — `Write` and
  `Execute` are gated, `Read` calls flow through untouched. This keeps the notion of
  "an edit" defined by the same permission ladder ADR-005 uses, not a hand-kept list
  of tool names that would drift as tools are added.
- **The approver is an injected callback, mirroring `stream_sink`.** `RunArgs` gains
  `edit_approver: Option<EditApprover>` — a `&(dyn Fn(&EditPreview) -> bool + Sync)`.
  The loop stays pure and testable: tests inject a closure that always rejects,
  always approves, or captures the preview; the CLI injects a stdin y/N prompt.
  `None` (the default everywhere else — chat/api/mcp/icm all pass `None`) means the
  gate is inert, so existing behavior is byte-for-byte unchanged.
- **Rejection is a first-class result the model can adapt to.** A vetoed call is
  *not* an abort and *not* silent — the loop synthesizes an error `ToolResult`
  (`"edit rejected by user"`, `is_error: true`) and feeds it back, exactly as if the
  tool had failed. The model sees the rejection in-context and can try a different
  approach; the run continues. This reuses the existing error-result plumbing rather
  than inventing a new control-flow path, so loop guards (no-progress, repetition)
  still see a coherent trace.
- **Preview is v1 (tool + targets + pretty args), diffs deferred.** `EditPreview`
  carries the tool name, the touched targets (pulled from the well-known arg keys —
  `path`/`from`/`to`/`src`/`dest`), and a pretty-printed JSON of the full args. The
  CLI approver prints this to **stderr** (so piped stdout stays clean), truncates the
  detail to 2000 chars, and reads y/N from stdin — empty, `n`, or EOF all reject
  (conservative default: if in doubt, don't write). A full unified-diff preview
  (rendering the before/after of a `write_file`/`edit_file`) is the natural next
  increment and is deferred.
- **Deferred:** unified-diff previews; an "approve all remaining" / session-sticky
  choice; accept-edits in the `chat` REPL (`/do` escalate path) — this increment is
  `query`-only.

## ADR-071 — 2026-07-23 (sprint 80): `fetch_reference` tool + ICM compose fetch mode — the Dark Matter knowledge-layer seam
The sibling project **Animus Dark Matter** (`crussella0129/Animus_Dark_Matter`)
formalized the ICM's Layer-3 reference plane as an on-demand, cached **knowledge
layer** rather than a pile of files folded whole into each stage's prompt (its
`SPEC.md` §6 / §11, `INTEGRATION.md`). This ADR is the Ferric side of that seam:
`ferric-icm` today reads a stage's `references/` and folds every file's full
content into the composed prompt (ADR-064) — for a large reference set that
dominates the context window and defeats the point of scoping. We add on-demand
retrieval so a stage pulls only the slice it needs.
- **A new `fetch_reference` tool, registered ONLY for ICM runs.** It reads the
  stage's own `references/`, chunks by ATX heading, keyword-scores against a
  `query`, and returns the top-k clean-markdown chunks (each headed by a `ref://`
  URI). It is `Read`/ring-0, but it is **not** in `register_builtin_tools` — `ferric
  icm run --fetch-references` registers it into that run's `config.registry`, so a
  normal `ferric query` never pays a tool-budget slot for it (ADR-028 rings).
- **Stage-contained scoping decides what is fetchable, not a heuristic.** A running
  stage is boundary-contained to `stages/NN_*` (ADR-065), so a tool can only reach
  that stage's own `references/`. Compose therefore un-folds **only** L3 inputs under
  the stage's `references/`; cross-stage L4 inputs (`../NN/output/`) and external L3
  (shared `../../_config`) are unreachable by the tool and so **stay folded**. The
  guard model, not a rule, defines the fetchable set.
- **Flag-gated (`ComposeMode`), default unchanged.** `compose_stage` keeps its exact
  behavior; `compose_stage_with_mode(.., FetchReferences)` is the new path, reached
  only via `--fetch-references`. Existing `ferric icm run` / `plan` are byte-for-byte
  unchanged. The flag also gives the SPEC-§9 validation its two arms for free
  (fold baseline vs fetch) — the plan is to A/B → tune → flip the default once proven.
- **Simple in-tree backend now; the DM MCP server is a drop-in later.** Retrieval is
  a recursive read + heading chunk + keyword score — deliberately dumb. A future Dark
  Matter stdio **MCP knowledge server** (semantic search, `ttlMs`/`cacheScope`
  caching, mirrored ingestion) replaces the backend behind the *same* `fetch_reference`
  contract; the model's grammar does not change. (Ferric has no MCP *client* yet, so
  a built-in tool is the cheaper seam than teaching the loop to consume an MCP server.)
- **Measured.** On a 133 KB reference vault, stage-1's assembled prompt dropped from
  136,162 chars (fold) to 3,355 chars (fetch) — **97.5% smaller** — with the model
  handed an "available references → `fetch_reference`" note instead. 10 new tests
  (8 tool, 2 compose-mode).
- **Deferred:** real-small-model iteration on fetch precision + task success (needs a
  live model), then flipping the default to fetch (the hard replace); wiring the DM
  MCP-server backend; a `--fetch-references` preview on `ferric icm plan`.

## ADR-072 — 2026-07-24 (sprint 82): verification is a boundary problem — four defects live inside a green suite
Sprint 81 audited all 14 crates by inspection and could not run one check: `C:`
was at 100% and `cargo test` wedged mid-link. It deliberately declined to write
an ADR, on the grounds that `decisions.md` is the durable record and should not
assert conclusions no test had confirmed. That was the right call, and this ADR
is what it deferred to. The user cleared `target/` (49 GB); every blocked check
ran; the findings are now verified rather than inferred. Full report:
`docs/verification-2026-07.md`.
- **The toolchain is green and that is real.** Cold `cargo build --workspace
  --all-targets`: clean, **0 warnings**, 31s. `cargo test --workspace`: **463
  passed / 0 failed** across 52 suites. `cargo clippy --all-targets`: **0
  warnings**. `cargo fmt --check`: clean. Dark Matter's `verify-spec.sh`: **PASS
  61 / FAIL 0**. The core is genuinely good — `ferric-guard/workspace.rs`,
  `ferric-core/scale.rs`, `ferric-tools/registry.rs`, the three loop guards.
- **And four defects live inside that green suite.** Each was made to fail a
  written test on current `main`: **A1** — ADR-002's 4,000-char tool-output
  truncation never reaches the model (`_for_model` is discarded at `run.rs:756`;
  the `ToolResult` event carries `full`). Measured: **20,028 chars** entered the
  context window on one `read_file`, a 5× budget overrun on every tier. **A3** —
  `Vcs::snapshot` runs `git add -A; git reset` **once per turn**, destroying the
  user's staged index (`staged.txt` → `""`). **A6** — `fetch_reference` drops
  tokens ≤ 2 chars, so `"Go"` finds nothing in a vault entirely about Go. **A2**
  — the taint set marks `digest.source` (harness-stamped provenance) while the
  untrusted `digest.summary` is what enters the prompt, inverting ADR-044's
  CaMeL-lite policy against its own threat model.
- **The structural lesson: each defect is covered up to a boundary and not
  across it.** `registry.rs:423` tests that the Registry *computes* the truncated
  view — and passes, because that end is correct; nothing tests that the loop
  *uses* it. `truncation_tests.rs` reads like coverage of A1 and is not (it tests
  cut-off model completions, an unrelated mechanism). `background_tasks.rs`
  covers `manage_task`'s happy path while every panic path stays untested. **Test
  strategy should follow the value across the crate seam, not stop at it.**
- **A code comment is not a design decision, and s81's endorsement of one was
  wrong.** `ferric-vcs/src/lib.rs:49` ships unresolved think-aloud ("Wait, `add
  -A` pollutes the staging area… we can use `git read-tree HEAD`"). Sprint 81
  repeated that suggestion as the fix. Measured in a scratch repo, `git read-tree
  HEAD` destroys the staged set **identically** to `git reset` — both reset the
  index to HEAD. The correct fix is a temporary `GIT_INDEX_FILE`, verified to
  preserve the user's index while still capturing untracked files in the tree.
- **The Dark Matter seam is two incompatible contracts sharing a name.** DM's
  `INTEGRATION.md` declares `required: ["target"]` with `query` optional; Ferric
  declares `required: ["query"]` and has no `target`. The DM-legal call from
  DM's own docs returns `Err("missing required string argument: query")`, and
  Ferric returns markdown where DM specifies `{chunks:[{uri,text,score}],
  truncated}`. DM's `test_ferric_citations_resolve` cannot see this: it checks
  only that two files *exist*, **neither of which is `fetch_reference.rs`**, and
  it `pass`es on skip when the Ferric repo is absent. **Decide `target` before
  DM's s2 build hardens the other side.**
- **Vestigial, proven not asserted.** The 6 unused dependencies (B3) were
  actually removed and `cargo check --workspace --all-targets` exited 0, then
  reverted. `Protocol::{FencedCode,EditFormat}` are never constructed;
  `LoopState.registry_tools` is never read; `SandboxConfig::default()` is never
  called. `TailnetFsRetriever` and `WebRetriever` remain unreachable from the
  binary (D1/D2), and with A2 unfixed the sink policy is inert on **every** path.
- **Scope: audit only, no remediation.** The tree is unmodified — probe tests
  were run, recorded, and removed. Remediation order is `docs/verification-2026-07.md`
  §8, led by A3 (silent data loss) then A1 (live context cost).

## ADR-073 — 2026-07-24 (sprint 83): remediating the sprint-82 audit — and what a fix has to prove
Sprint 82 verified the codebase and left four demonstrated defects plus a
vestigial list. This sprint fixes them. The rule applied throughout: a fix is not
done when the code looks right, it is done when the test that failed before
passes and the reason it failed is written down. Two of these fixes would have
been wrong if that rule had been relaxed. Workspace went 463 → **478 tests, 0
failures**, clippy and fmt clean.
- **A3 — the snapshot no longer touches the user's index, and no longer escapes
  the workspace.** `Vcs::snapshot` staged with `git add -A` then `git reset`,
  once per turn, destroying a staged index on turn 1 and every turn after. It now
  stages into a private `GIT_INDEX_FILE` seeded from the real index (so `add -A`
  keeps its stat cache). **Sprint 82's recommended fix was wrong and measuring it
  is what caught that:** `git read-tree HEAD`, which the shipped source comment
  proposed, resets the index to HEAD exactly like `git reset`. Not touching the
  real index is the only correct answer.
- **A3, second half — a containment bug the audit missed entirely.** Git
  discovery walks *upward*, so a workspace that is not itself a repo resolves to
  the nearest ancestor repo. On this machine `~` **is** a git repo, so a plain
  temp dir resolves to toplevel `C:/Users/charl` — meaning the per-turn
  `git add -A` targeted the user's entire home directory, and `revert` would have
  run `git clean -fd` across it. `snapshot`/`revert`/`untracked_to_be_removed`
  now refuse unless the workspace root *is* the worktree root
  (`VcsError::NotWorkspaceRoot`, which `run.rs` already degrades gracefully).
  Empirically this took `cargo test -p ferric-cli` from wedging past 10 minutes to
  1 second. **Found by writing a test for something adjacent to the known bug** —
  the "no temp index left behind" test used an awkward session id, which
  surfaced the unsanitized ref name, which led here.
- **A3, third half — `revert` now confirms.** It deletes untracked files and
  truncates the trace; neither was announced. The prompt lists the actual doomed
  paths from a `git clean -nd` dry run (`--yes` to skip).
- **A1 — truncation restored where the context window is actually built.** The
  4,000-char view was computed by the Registry and dropped by the loop; 20,028
  chars reached the model on one `read_file`. The **projector** now applies it,
  which keeps run and replay identical by construction rather than by parallel
  maintenance, and the projector's limit is seeded from
  `Registry::truncation_limit()` — making `with_truncation_limit` reach the model
  for the first time. 4 tests covering **both** halves of the contract, because
  testing one half is how this was lost.
- **A2 — taint the content, and at a granularity that can match.** The live path
  tainted `digest.source` (harness-stamped, trusted) while injecting
  `digest.summary` (untrusted), inverting ADR-044 in both directions at once.
  **Correcting only the value would have produced a fix that never fires:**
  `is_tainted` needs the needle *inside* the argument, so tainting the whole
  summary misses the realistic attack — the model lifting one injected sentence
  into a `write_file`. New `TaintSet::taint_text` tags the block plus each line
  and sentence, with a 12-char floor (below it, needles match everything and the
  policy denies everything). Still inert everywhere except
  `ferric query --research`, which remains the only taint source (D3).
- **A6 — short query terms work, without reintroducing the noise they were
  dropped for.** `"Go"` returned nothing over a vault entirely about Go. Deleting
  the length filter alone would have been wrong: scoring is substring-based, so
  `"go"` matches `"algorithm"` — which is *why* the filter existed. Terms under 3
  chars now match whole words; longer terms keep substring matching, preserving
  stem search. Also fixes a byte-vs-char length bug in the same predicate.
- **Vestigial code cleared, verified unreachable first.** Six unused deps removed
  (proven by building without them), `LoopState.registry_tools` and its
  `#[allow(dead_code)]` deleted, the duplicate `test-sweep-prompt.txt` removed,
  `protocol-unified-grammar.md` renamed to `protocol-text-xml.md`. Two were
  resolved by **use** rather than deletion, which was the better answer:
  `SandboxConfig::default()` is now called by `WebRetriever::new` instead of
  duplicated, and `_parse_error` is surfaced as a `Note` so a grammar failure
  stops being indistinguishable from an empty completion in the trace.
- **Deferred, deliberately:** A5 (invert the sandbox airlock to opt-out), A7
  (wire `RequireApproval` to the `EditApprover` shipped in ADR-070), A4 (de-panic
  `manage_task`), C1 (`run_with_provider(RunArgs)`), and the Dark Matter `target`
  contract decision. A5 and A7 both touch `WebRetriever`/sink wiring that is still
  unreachable from the binary (D1/D2); doing them properly means wiring, not
  patching, and that deserves its own sprint.

## ADR-074 — 2026-07-24 (sprint 84): finishing the audit — panics, argument lists, and defaults that were the wrong way round
Sprint 83 fixed the four defects the audit had *demonstrated*. This sprint clears
what it deferred: A4, A5, A7, C1–C5, and the Dark Matter contract. Workspace
487 → **503 tests, 0 failures**, clippy and fmt clean. Two defects that were in no
report surfaced along the way, both found by testing something adjacent.
- **A4 — the one model-invokable tool that could kill the harness.** `manage_task`
  held 12 lock `.unwrap()`s; one panicking task thread poisons a lock and every
  later call aborts the process. All 12 gone: accessors recover a poisoned guard
  with `into_inner()`, which is safe because the guarded data (a status enum, a
  `Child` handle) has no invariant a panicking writer could half-break. The two
  runtime panics (`Handle::current()` with no runtime, `block_in_place` on a
  current-thread runtime — while ferric-loop is deliberately executor-agnostic)
  become ordinary tool errors via a new `builtin::blocking::block_on_ambient`.
- **A4's spread: `shell_exec` had the same bug, and it matters more.** Writing the
  test for `manage_task` found the identical `block_in_place` +
  `Handle::current()` pair in a **Ring-0** tool reachable from far more paths.
  Checked the blast radius before changing it: every in-process tool path builds
  `Runtime::new()` (multi-thread), and `cron` only *looks* current-thread because
  it spawns jobs as subprocesses. Nothing that worked before changed.
- **A NEW defect, not in any report: background-task ids collided.**
  `format!("task-{millis}")` is not unique; two tasks started in the same
  millisecond got the same id, and since the registry is keyed by id the second
  **silently evicted the first** — losing its `Child` handle, leaving the task
  unlistable, uninspectable, unkillable. It surfaced as two tests in
  `background_tasks.rs` flaking against each other, which I had earlier written
  off as harmless cross-test interference. **That write-off was the mistake**; the
  flake was the bug reporting itself. Ids now carry a monotonic counter.
- **A7 — two human-approval systems, introduced at last.** `RequireApproval`
  degraded to a flat denial commenting "human approval is not wired", while
  ADR-070's `EditApprover` had been sitting at the very dispatch site that calls
  `execute` for four sprints. Wired through a callback `ferric-tools` owns
  (`ApprovalRequest`/`SinkApprover`), so the chokepoint can ask a human without
  depending on the loop. With no approver it still denies — the safe reading of
  "require approval" when nobody can approve — but now says so.
- **A5 — the airlock is opt-out, not opt-in.** `SandboxConfig::default()` paired
  `--network bridge` with no proxy and no gVisor: dropped capabilities and
  **unrestricted egress**, for the component whose job is running untrusted
  retrieval. Default is now no network + gVisor required (a missing `runsc` fails
  closed, the correct direction), and `Option<proxy_url>` becomes a
  `NetworkPolicy` enum so unrestricted egress is a variant someone must *write*
  and a reviewer can grep for. Docker is absent on this machine, so rather than
  ship it untested the argv construction — the security-relevant part — is split
  into a pure `docker_args()` and tested directly.
- **C1 — 18 positional parameters, gone.** `run_with_provider` re-packed its 18
  arguments into `RunArgs`, a struct that already existed, behind five
  `too_many_arguments` allows (now 1, on an unrelated function). A `LoopSetup`
  carries everything except the provider — the one thing `drive_real` can only
  build inside its own runtime. Knock-on: `icm.rs` had a `macro_rules!` whose
  comment says outright it existed because the argument list had to be written
  twice; it is gone.
- **C2/C3/C4/C5 —** the `post_turn` block (copy-pasted at all four turn exits) is
  one method; `ferric-vcs` is honestly synchronous instead of `async fn` with no
  `.await` (which under tokio silently blocked a reactor thread per turn while
  looking like it didn't), dropping its last tokio dependency; the task registry
  gains the removal path it never had; the duplicated status match is one method.
- **Dark Matter: the call shape agrees now; the return shape is still a decision.**
  Ferric accepts `target` and no longer requires `query`, so the call written in
  DM's own INTEGRATION.md works. Silent `k`-capping now reports how many chunks
  were withheld. **Deliberately not changed:** DM SPEC §6.2's
  `{chunks:[{uri,text,score}], truncated}` envelope vs Ferric's markdown —
  flipping that changes what every small model sees and would invalidate
  ADR-071's measured 97.5% reduction, so it wants a measurement behind it.
- **DM's verifier can see the seam now.** `test_ferric_citations_resolve` asserted
  only that two files exist, **neither of them `fetch_reference.rs`** — it would
  have passed if the tool had never been written, and it did pass throughout the
  divergence. A new check reads the actual descriptor. Testing the check itself
  caught two ways it lied: a `"required": ["query"]` grep false-positives on the
  legitimate `anyOf` branch, and a whole-file grep false-negatives because a test
  mentions the field name. Verified with a negative control in both directions.
  Also, a check that cannot run now reports `skip` rather than calling `pass`.
- **The through-line of sprints 82–84:** a green suite is evidence about what is
  tested, not about what works. Every defect in this ADR was invisible to 487
  passing tests, and two of them were found only by writing a test for something
  *next to* the known bug.

## ADR-075 — 2026-07-25 (sprint 85): round-2 verification — auditing your own recent work first
A second full-codebase verification from a cold clean-room build, weighted
deliberately toward sprints 83–84's own changes rather than spread evenly. That
weighting is the finding: **three of the four new defects were introduced by the
remediation sprints themselves, and two of those sit in the security-facing code
those sprints existed to fix.** Full report: `docs/verification-2026-07-round2.md`.
- **The baseline is clean-room green.** Cold `cargo build --workspace
  --all-targets`: 0 warnings, 41s. `cargo test --workspace`: **503 passed / 0
  failed / 2 ignored** across 53 suites. Clippy 0, fmt clean, Dark Matter
  verifier PASS 62 / FAIL 0 / SKIP 0.
- **E1 (PROVEN, ours) — one tool call prompts the human twice.** Sprint 84 gave
  the sink gate the same `EditApprover` the accept-edits gate already uses, so
  `--accept-edits` + `--sink-action requireapproval` + tainted args fires both.
  Measured: `approver_prompt_count=2`. Worse than the annoyance: approving at one
  gate and rejecting at the other is behaviour nobody designed.
- **E2 (PROVEN, ours) — the taint granularity makes `--research` unusable, and no
  threshold fixes it.** Sprint 83 chose `MIN_TAINT_SEGMENT_CHARS = 12` by
  judgement. Measured against a realistic digest, **3 of 3 faithful restatements
  of researched material are blocked** under the default `Deny`. The mechanism —
  substring taint — **cannot distinguish "copied an injected instruction" from
  "wrote a true fact it learned"**; both are literal text from the digest.
  Lowering the floor worsens false positives, raising it readmits the lifted
  sentence sprint 83 added `taint_text` to catch. **This is a posture decision,
  not a tuning bug**, and it is recorded as one: default the research path to
  `Warn`, taint only instruction-shaped fragments, or document `--research` as
  read-mostly under `Deny`. Sprint 83's direction (taint content, not the trusted
  provenance label) stands.
- **E3 (CONFIRMED, ours) — run and replay disagree about the cap.** ADR-074
  asserted the projector keeps run and replay "identical by construction".
  `run.rs:560` seeds from `Registry::truncation_limit()`; `replay.rs:63` and
  `compact.rs:224` use the default. Latent only because
  `with_truncation_limit` still has no non-default caller. **The ADR-074 wording
  was an over-claim** and is corrected here rather than left standing.
- **E4 (CONFIRMED, not ours) — `ferric chat` discards trace-write failures** at
  all 6 sites while `run.rs` propagates at 21. `write_event` can genuinely fail
  on I/O, so a chat trace can be silently incomplete — in a project whose thesis
  is "if it isn't in the trace, it didn't happen".
- **Three defect classes are now closed, and that is worth stating.** Production
  `unwrap`/`expect` across tools/loop/guard is down to 6 sites, all provably safe
  idioms (constant regex; capture groups guaranteed by a successful match;
  `take()` right after `Stdio::piped()`). No `not wired`/TODO/FIXME intent
  comments remain. All seven pre-audit backlog entries still cite live files.
- **The bounding gap: nothing has met a real model since ~sprint 26.** All 503
  tests are mock-driven; A5's sandbox has never run against Docker; A2's and A6's
  thresholds are asserted, not measured against real digests or vaults. **A suite
  this green, this long without a live run, is measuring the mocks as much as the
  code.** A live-model round is now the highest-value next investment — above
  C7/C8/B1 combined.
- **A process finding.** C7, C8 and B1 came out of sprint 82 but were never
  entered in `agent-tasks/` — they lived only in a README "Next" line, and went
  three sprints unpicked-up as a result. Prose is not a ledger; they are entered
  now.
- **Scope: audit only.** The tree is unmodified — probes were run, recorded and
  deleted, and the suite is green afterwards. Remediation order is in the
  report's §5, led by E1 (a regression we introduced) and E4.

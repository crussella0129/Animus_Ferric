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

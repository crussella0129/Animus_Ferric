<p align="center">
  <img src="docs/logo.jpg" alt="Animus Ferric — agentic AI harness written in Rust" width="720">
</p>

# Animus Ferric

A local-first agentic coding harness written in Rust, purpose-built for **small local models (1B–14B GGUF)**.

Ferric is the Rust synthesis of the Animus lineage — [Animus](https://github.com/crussella0129/Animus) (Python), [Animus_Prion](https://github.com/crussella0129/Animus_Prion) (Go), and [fev](https://github.com/crussella0129/fev) (Go) — built on three convictions:

1. **The harness should own decoding.** Constrained generation (JSON-Schema / regex / CFG grammars) is driven end-to-end in the agent loop, so malformed tool calls are *impossible* rather than repairable.
2. **Behavior should scale to the model, deterministically.** A pure function maps a model profile (params, quant, context, measured capability level) to a run policy (protocol, plan granularity, turn budgets, tool count). Small models get small steps.
3. **The trajectory is the source of truth.** Every session writes a versioned JSONL trace — full conversation, tool calls, untruncated tool output, execution chain — replayable and diffable. If it isn't in the trace, it didn't happen.

Rust is not an implementation detail: the visible, demonstrable chain of ownership over state and execution is part of how you verify you control the agent.

## Workspace

| Crate | Responsibility |
|---|---|
| `ferric-core` | Shared types + the deterministic scale function (ModelProfile → RunPolicy) |
| `ferric-trace` | Versioned TraceEvent schema, flush-per-event JSONL sink, tolerant reader |
| `ferric-provider` | Async `Provider` trait (constraint-carrying) + deterministic mock; real backends behind feature flags |
| `ferric-guard` | Hardcoded security: workspace boundary, permission checker, deny lists |
| `ferric-tools` | Tool trait, registry chokepoint, builtin file tools |
| `ferric-cli` | The `ferric` binary |

## Build

You need a recent stable Rust toolchain (`rustup`, edition 2024). Inference backends are **feature-gated** — pick the one(s) you want:

```sh
git clone https://github.com/crussella0129/Animus_Ferric.git
cd Animus_Ferric

# Recommended: the OpenAI-compatible HTTP valve — the constrained-decoding path.
# Talks to llama.cpp (llama-server), Ollama, or vLLM.
cargo build --release -p ferric-cli --features backend-openai

# Optional: also the in-process mistral.rs GGUF backend (text-only TextXml path).
cargo build --release -p ferric-cli --features backend-openai,backend-mistralrs
```

The binary lands at `target/release/ferric` (`ferric.exe` on Windows). Built with **no** backend feature, only the trace tooling works; `query`/`toolbench` will tell you to rebuild with a feature. Examples below assume `ferric` is on your `PATH`.

## Using Ferric

| Command | What it does |
|---|---|
| `ferric server up\|status\|doctor\|down` | Launch & manage the local OpenAI-compatible inference server (the HTTP valve), bound to `127.0.0.1` only. Writes `.ferric/server.json` so other commands auto-discover it. |
| `ferric query "<prompt>"` | Run one workspace-scoped agent turn against a local model. |
| `ferric toolbench` | Measure & diagnose tool-calling fire rate for a model (or a fleet — see below). |
| `ferric bench` | Run the L0–L6 capability ladder and calibrate a model's `measured_level`. |
| `ferric trace cat <file.jsonl>` | Render a session trace as a human-readable log. |

A typical loop — bring a server up, point Ferric at it, work, tear it down:

```sh
ferric server up --engine ollama --model qwen2.5-coder:7b   # or --engine llama-server --model your.gguf
ferric server status                                        # prints base URL + health
ferric query "list the Rust files and summarize lib.rs"     # auto-discovers the server
ferric server down
```

`query` and `toolbench` **auto-discover** the running server from `.ferric/server.json` — no `--api-base` needed. To target a server you didn't launch (e.g. an already-running Ollama), pass `--api-base http://localhost:11434/v1`. By default `query` runs the **constrained** path, which is the reliable one for small models.

`ferric query` also takes **any file** as input with `--file` (repeatable): text/code files fold into the prompt (works on any model), while image/audio/video attach as content parts when you declare `--modality` and the model can read them (Gemma 3n on the OpenAI valve). See [docs/multimodal.md](docs/multimodal.md).

## Test it with your own models

Ferric works with large and small models alike, but *how well* a small model drives the tools varies — so don't take it on faith, **measure it**. The testbench runs every tool many times, classifies *why* it misses, and grades the result, so you can dial a model down until quality drops.

**1. Bring a model.** Any of:

```sh
# Ollama — pull whatever you want to test:
ollama pull qwen2.5-coder:7b

# llama.cpp — point the launcher at a GGUF on disk (needs `llama-server` on PATH):
ferric server up --engine llama-server --model /path/to/your-model.gguf [--mmproj mmproj.gguf] [--ctx 8192]

# In-process GGUF (mistral.rs, text-only path) — no server needed:
ferric toolbench --backend mistral --model-dir /path/to/models --model-file your-model.gguf
```

**2. Benchmark it** under the constrained path, and write a report:

```sh
ferric toolbench --backend openai --model <name> --protocol grammar --iterations 20 --report report.md
```

**3. Read the verdict.** `report.md` gives each tool a success rate, a **failure taxonomy** (`wrong_tool` / `malformed_args` / `no_action` / `parse_error`), and an acceptability band — **solid** (≥90%) / **marginal** (≥70%) / **unreliable** (<70%). Add `--protocol native` to compare the unconstrained path on the same model.

**4. Calibrate a whole fleet at once.** `--models <a,b,c>` benches each model and ranks them into one leaderboard, sorted best→worst — so you can pick the smallest model that's still *solid*:

```sh
ferric toolbench --backend openai --models qwen2.5-coder:7b,llama3.1:8b,llama3.2:1b --protocol grammar --report fleet.md
```

```
# Fleet Leaderboard
| Model              | Protocol        | Success | Rate   | Verdict |
|--------------------|-----------------|---------|--------|---------|
| qwen2.5-coder:7b   | ConstrainedJson | 50/50   | 100.0% | solid   |
| llama3.1:8b        | ConstrainedJson | 50/50   | 100.0% | solid   |
| llama3.2:1b        | ConstrainedJson | 50/50   | 100.0% | solid   |
```

That run is real: the constrained path holds at **100% down to a 1B model**, where the same model's *native* tool-calling collapses to 22% — which is the whole point of harness-owned decoding. Full walkthrough: [docs/testbench.md](docs/testbench.md).

## Portability

CPU-first. The baseline target includes Raspberry Pi / Orange Pi class aarch64 hardware; CI gates `cargo check --target aarch64-unknown-linux-gnu`. CUDA (NVIDIA, Jetson) and AMD paths are planned as specialized backends.

## Status

Active development (sprint 11). Two inference backends ship behind feature flags:

- **`backend-openai`** — an OpenAI-compatible HTTP valve (llama.cpp / Ollama / vLLM) that enforces a harness-authored JSON-Schema constraint server-side. This is the constrained-decoding thesis working for small GGUF models — out-of-process, with pure Rust on Ferric's side. **It's the default and the reliable path.**
- **`backend-mistralrs`** — in-process mistral.rs GGUF, driven text-only via the loop's `TextXml` protocol. Sprint 11 wired its `set_constraint` and probed it: mistralrs 0.8.15 still **hangs** llguidance on GGUF even for a trivial schema (ADR-027), so the constrained path stays off here — it remains the unconstrained fallback.

The action protocol (`NativeTools` / `ConstrainedJson` / `TextXml`) is chosen from each backend's real capabilities. An embedded PyO3/PyTorch backend was tried and removed (ADR-021) — external engines are reached only via the out-of-process valve. Development follows a sprint-loop protocol; see `decisions.md` for ADRs and `agent-tasks/` for the ledger.

## Development timeline

Ferric is built in **sprints** — a Research → Plan → Build → Test → Loop protocol. The durable record lives in `decisions.md` (the full ADR log) and `agent-tasks/` (the task ledger); this is the human-readable summary. *Newest last — append the next sprint here as it closes.*

- **Sprint 0 — Foundations** (2026-06-10). Cargo workspace + six crates; the deterministic scale function (ModelProfile → RunPolicy); versioned JSONL trace; hardcoded security (workspace boundary, deny lists); builtin file tools; CLI stub. *ADR-001–009.*
- **Sprint 1 — The agent loop** (2026-06-11). Turn loop with policy budgets, a structured task-complete terminator, a repetition guard, and retry backoff; a command structure with no chat catch-all. *ADR-010–014.*
- **Sprint 2 — Action grammar & calibration** (2026-06-13). The unified `ActionProtocol` grammar; per-tier output-token budgets; the `bench` L0–L6 capability ladder as the sole producer of `measured_level`. The server-side constrained-decoding **hang** surfaces and is quarantined opt-in. *ADR-015–020.*
- **Sprints 3–6 — Exploration** (mid-June 2026). An embedded PyO3/PyTorch inference path and a first-generation toolbench. This era drifted from the *harness-owns-decoding* thesis — and set up the realignment.
- **Sprint 7 — The realignment** (2026-06-23). The PyO3/PyTorch backend removed; external engines reached only through the out-of-process HTTP valve. The constraint reinstated, capabilities made honest, and the `NativeTools` / `ConstrainedJson` / `TextXml` trichotomy chosen from each backend's *real* capabilities; toolbench rebuilt around the active protocol. *ADR-021–023.*
- **Sprint 8 — Launcher + testbench** (2026-06-23). The `ferric server` lifecycle manager (llama-server default, Ollama pluggable, runfile auto-discovery) and the diagnostic toolbench (failure taxonomy + verdict bands). **Thesis proven on a real model: constrained 100% vs native 0% on the same Ollama model.** *ADR-024.*
- **Sprint 9 — Fleet calibration** (2026-06-23). `ferric toolbench --models` sweeps a fleet into one sorted leaderboard. **The constraint holds 100% down to a 1B model where native collapses to 22%** — it extends the usable model floor to 1B. A native-`content` fallback closes the "Ollama returns the call as text" gap; the mistral.rs 0.8.15 probe confirmed the hang is fixed upstream but the constraint still isn't enforced. *ADR-025.*
- **Sprint 10 — Multimodal "any file" input** (2026-06-24). `ferric query --file` takes any file: text/code folds into the prompt (any model); image/audio/video attach as OpenAI content parts, capability-gated by `--modality` + the backend's `supports_media` (the valve carries media; the in-process path doesn't). Additive `Message.media` (media-free messages serialize unchanged); a dependency-free base64 encoder. The pure pipeline is fully unit-tested; the live-media heartbeat (a real model reading a clip) is deferred until a multimodal server is stood up.

- **Sprint 11 — mistral.rs constrained-decoding spike** (2026-06-24). Settled an open question: `MistralRsProvider` had been *stripping* the decoding constraint since the s3 pivot, so the sprint-9 probe (ADR-025) had measured the stripped path, not enforcement. Wired the constraint through (`set_constraint`) and re-probed — mistralrs 0.8.15 **still hangs** llguidance on GGUF even for a trivial schema (5-minute engine timeout). The ADR-020 hang is *not* fixed; the wiring was reverted (no regression), mistral.rs stays text-only, and the HTTP valve remains the sole constrained path. *ADR-027.*

> **Next — Sprint 12: TBD** (the deferred live-media heartbeat once a multimodal server exists; or a new direction from the next research phase — e.g. MCP-stdio integration, ADR-012).

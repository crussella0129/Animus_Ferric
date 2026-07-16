<p align="center">
  <img src="docs/Animus.png" alt="Animus Ferric — agentic AI harness written in Rust" width="720">
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
```

The binary lands at `target/release/ferric` (`ferric.exe` on Windows). Built with **no** backend feature, only the trace tooling works; `query`/`toolbench` will tell you to rebuild with a feature. Examples below assume `ferric` is on your `PATH`.

## Using Ferric

| Command | What it does |
|---|---|
| `ferric server up\|status\|doctor\|down` | Launch & manage the local OpenAI-compatible inference server (the HTTP valve), bound to `127.0.0.1` only. Writes `.ferric/server.json` so other commands auto-discover it. |
| `ferric query "<prompt>"` | Run one workspace-scoped agent turn against a local model. |
| `ferric mcp` | Run an MCP-stdio server exposing one tool, `ferric_query`, to MCP clients (Claude Code, Cursor, an IDE). Workspace/backend/model are launch-time-fixed flags; each call runs the full constrained agent loop. *ADR-046.* |
| `ferric toolbench` | Measure & diagnose tool-calling fire rate for a model (or a fleet — see below). |
| `ferric bench` | Run the L0–L6 capability ladder and calibrate a model's `measured_level`. |
| `ferric trace cat <file.jsonl>` | Render a session trace as a human-readable log. |

A typical loop — bring a server up, point Ferric at it, work, tear it down:

```sh
ferric server up --engine llama-server --model your.gguf
ferric server status                                        # prints base URL + health
ferric query "list the Rust files and summarize lib.rs"     # auto-discovers the server
ferric server down
```

> [!NOTE]
> If you run `ferric server up --tailscale` on a machine for the first time, you must authorize Tailscale Serve on your Tailnet. The command will output an authorization link that you must click to unblock the proxy and register the server globally.

`query` and `toolbench` **auto-discover** the running server from `.ferric/server.json` (or your global `APPDATA` directory) — no `--api-base` needed. To target a server you didn't launch (e.g. an already-running Ollama), pass `--api-base http://localhost:11434/v1`. By default `query` runs the **constrained** path, which is the reliable one for small models.

`ferric query` also takes **any file** as input with `--file` (repeatable): text/code files fold into the prompt (works on any model), while image/audio/video attach as content parts when you declare `--modality` and the model can read them (Gemma 3n on the OpenAI valve). See [docs/multimodal.md](docs/multimodal.md).

Add `--stream` to see text live as it's generated instead of waiting for the whole multi-turn loop to finish (opt-in; ADR-047).

**Builtin tools** (all workspace-scoped and security-checked through the guard). The always-on **core** (Ring 0) is the navigate/mutate set: `read_file`, `list_dir`, `write_file`, `make_dir`, `edit_file` (surgical first-occurrence replace — small models do targeted edits far more reliably than full rewrites), and `delete_path` (file or, with `recursive`, a directory). **Ring 1** ("find & organize") adds four: `search_files` (grep-style *content* search → `relpath:lineno:line`), `find_files` (find files by *name* → relpaths), `move_path` (move/rename), and `copy_file` — so a small model can locate code and reorganize it once it's proven the core. **Ring 2** ("plan & apply structured changes") seeds with `multi_edit` — an ordered, *atomic* batch of edits to one file (all-or-nothing; reachable once a model earns the Medium tier). Tool vocabularies are organized as **rings** that widen as a model proves it can call them reliably, with the active rings forming the constrained grammar. `ferric query --max-ring 0` pins any model to the Ring-0 core — the smallest, surest grammar — regardless of its size (restrict-only; to *widen* a small model's rings, prove it with the toolbench so `measured_level` promotes it).

## Test it with your own models

Ferric works with large and small models alike, but *how well* a small model drives the tools varies — so don't take it on faith, **measure it**. The testbench runs every tool many times, classifies *why* it misses, and grades the result, so you can dial a model down until quality drops.

**1. Bring a model.** Any of:

```sh
# llama.cpp — point the launcher at a GGUF on disk (needs `llama-server` on PATH):
ferric server up --engine llama-server --model /path/to/your-model.gguf [--mmproj mmproj.gguf] [--ctx 8192]
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

That run is real: the constrained path holds at **100% down to a 1B model**, where the same model's *native* tool-calling collapses to 22% — which is the whole point of harness-owned decoding.

**5. Calibrate the rings.** `--calibrate-rings` benches a model **ring by ring** and reports the highest ring it reliably drives — the recommended `--max-ring` to run it at (`ferric toolbench … --calibrate-rings`). It's the demonstrated-reliability promotion: a model *earns* a wider grammar by proving it on the bench. Full walkthrough: [docs/testbench.md](docs/testbench.md).

## Portability

CPU-first. The baseline target includes Raspberry Pi / Orange Pi class aarch64 hardware; CI gates `cargo check --target aarch64-unknown-linux-gnu`. CUDA (NVIDIA, Jetson) and AMD paths are planned as specialized backends.

## Status

Active development (sprint 44). The single inference backend used by Ferric:

- **`backend-openai`** — an OpenAI-compatible HTTP valve (`llama.cpp` / vLLM) that enforces a harness-authored JSON-Schema constraint server-side. This is the constrained-decoding thesis working for small GGUF models — out-of-process, with pure Rust on Ferric's side. **It's the default and the reliable path.**

The action protocol (`NativeTools` / `ConstrainedJson` / `TextXml`) is chosen from the backend's real capabilities. Development follows a sprint-loop protocol; see `decisions.md` for ADRs and `agent-tasks/` for the ledger.

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

- **Sprint 12 — `search_files` tool** (2026-06-24). Added the missing content-search primitive a small coding agent leans on most: a workspace-scoped, guard-checked, dependency-free substring search (`relpath:lineno:line`, sorted + capped, binary/noise-dir skipping) gated at `Nano` so every model gets it. Mirrors the `list_dir` pattern; six temp-workspace tests.

- **Sprint 13 — complete Ring 0** (2026-06-24). Added the two missing core tools — `edit_file` (surgical replace) and `delete_path` (guard-scoped, `recursive`-gated) — then re-ran the toolbench to *measure* that the full navigate/mutate core still fires reliably. Introduces the **tool-rings** model: a curated core that widens as a model proves itself, with the active rings = the grammar.

- **Sprint 14 — formalize the rings** (2026-06-24). Made the tool-rings model real: every tool declares a `ring` (0 = the navigate/mutate core), `ring_for_tier` sets the capability ceiling (and honours `measured_level`, so reliability — not size — widens the set), and `tools_for_policy` **trims from the outer ring first** so the core is never dropped. Fixes the latent alphabetical `max_tools` cap. The active rings literally *are* the constrained grammar. *ADR-028.*

- **Sprint 15 — `--max-ring` override** (2026-06-24). The explicit "control exactly which rings" lever: `ferric query`/`toolbench --max-ring N` caps the active rings independent of tier (`--max-ring 0` = the core-only grammar). Restrict-only — widening past a model's capability stays earned via `measured_level`. Proven end-to-end via the trace's offered-tools. *ADR-028 (amended).*

- **Sprint 16 — ring calibration** (2026-06-24). `toolbench --calibrate-rings` sweeps a model ring-by-ring and reports the highest ring it reliably drives — the recommended `--max-ring`. Closes the rings loop: a model *earns* a wider grammar by proving it on the bench (the demonstrated-reliability promotion). *ADR-028.*

- **Sprint 17 — durable promotion** (2026-06-25). Closed the profile read-back loop: `model_profiles.json` was written by `ferric bench` but never read. Now `toolbench --calibrate-rings --profile-dir` *persists* the earned ring, and `ferric query --profile-dir` *reads the profile back* — a proven model auto-runs at its earned tier (`measured_level`) and ring (`calibrated_ring`), no manual flag. Safe no-op without a profile. *ADR-029.*

- **Sprint 18 — round out Ring 1** (2026-06-25). Added `find_files` (find by *name*, the companion to `search_files`' content search) and `copy_file` (the organize complement to `move_path`), making Ring 1 a coherent four-tool "find & organize" set. Both fire `solid` in the re-bench — growing the ring didn't cost reliability. *ADR-028 (amended).*

- **Sprint 19 — seed Ring 2** (2026-06-25). Added `multi_edit` (`ring: 2`) — an ordered, atomic batch of first-occurrence edits to one file (more than the Ring-0 `edit_file`, still reliably emittable vs a unified diff). Added `toolbench --params-b` so calibration can bench at a chosen tier and reach Ring 2 (`--params-b 20` → Medium → rings 0–2). *ADR-028 (amended).*

- **Sprint 20 — the full agentic loop, validated on a real model** (2026-06-26). The L0–L6 ladder (`ferric bench`) tests multi-turn *task completion*, but its runner could only reach `--mock` or the mistral GGUF backend (which hangs under constraint). Wired the openai backend through (`bench --backend openai`), fixed a verification bug it surfaced (the `task_complete` terminator wasn't credited), and ran it: **qwen2.5-coder:7b passes all of L0–L6 on the constrained path → `measured_level 6` (Small→Large)**. The first end-to-end validation that the constrained multi-turn loop completes real tasks, not just single tool calls — and the demonstrated-reliability promotion now runs on real data. *ADR-030.*

- **Sprint 21 — fleet agentic capability map** (2026-06-26). `bench --models` runs the full L0–L6 loop across the fleet and prints a `measured_level` leaderboard. The map: **qwen2.5-coder:7b → 6 (Large); llama3.1:8b → 5 (Medium); llama3.2:1b → none (fails L0)**. The honest finding: a 1B fires single tool calls at 100% but **can't complete a multi-turn task** — single-shot reliability ≠ agentic capability; and the code-tuned 7B beats the larger general 8B. *ADR-030 (amended).*

- **Sprint 22 — why the 1B isn't an agent** (2026-06-26). Diagnosed (from the trace) *why* `llama3.2:1b` fails L0: **repeat-not-terminate** (it re-calls `list_dir` instead of `task_complete`) and **semantic flailing** (15 `make_dir`s, no progress). Sharpened the repetition nudge into a direct imperative — but it **didn't move the 1B** (still `measured_level: none`), so the ceiling is a real capability limit, not wording. The nudge ships anyway (helps mid-tier models, can't regress capable ones). *ADR-031.*

- **Sprint 23 — llama.cpp first-class** (2026-06-26). Validated Ferric on full **llama.cpp** (`llama-server`) for the first time — we have now fully removed inference from ollama and switched exclusively to `llama.cpp` due to speed. The constrained loop runs on it at **100% Ring-0 tool-call fire rate**, with a context window as wide as you want (`-c`), the multimodal path (`--mmproj`), and a single edge-ready binary (Jetson / Pi). Guide: [docs/llama-cpp.md](docs/llama-cpp.md). *ADR-032.*

- **Sprint 24 — multimodal goes live** (2026-06-26). Ran an image end-to-end for the first time (the marquee goal deferred since sprint 10): a generated red square → `ferric query --file --modality image` → `llama-server --mmproj` (SmolVLM-500M) → the vision encoder processed it, and the model correctly answered **"Red."** The `image_url`/base64 content-parts mapping is proven against real pixels; no Ferric code change needed. Caveat: a sub-1B VLM degrades under the JSON grammar (use a bigger VLM or an unconstrained describe). *ADR-033.*

- **Sprint 25 — Gemma 4 E4B is the reference model** (2026-06-27). The data shows a **~4B agentic floor** (1B → none, 7B → 6, 8B → 5), so the fix for small models isn't a workaround — it's a *capable* one. Validated **Gemma 4 E4B** (4B, multimodal, function-calling) on llama.cpp: **`measured_level 5`** (matches the 8B), Ring-0 toolbench **100% solid**, and it **describes an image *inside* the constrained agentic loop** (`task_complete("a solid red rectangle")`) — closing the ADR-033 caveat with no harness change. Edge-feasible q4. *ADR-035.* (Use a GPU build for speed — CPU timed out the simplest level.)

- **Sprint 26 — audio modality** (2026-06-27). Validated **audio** end-to-end (the other half of multimodal): a Windows-TTS speech clip → `ferric query --file speech.wav --modality audio` → `llama-server` (Gemma 4's Conformer encoder) → **`task_complete("The quick brown fox jumps over the lazy dog.")`** — an exact transcription *inside* the constrained agentic loop. So Ferric multimodal is now **vision + audio**, both live on the reference model. No Ferric code change (the `input_audio` mapping was already correct). *ADR-036.*

- **Sprint 27 — no-progress guard** (2026-06-27). Closed ADR-031's second, still-unguarded failure mode — **"semantic flailing"** (the model calls the same tool with *different* args every turn and never completes, grinding to `max_turns`). The repetition guard misses it by design (it hashes name **+ args**); the new `ProgressGuard` tracks the same-tool-**name** streak (arg-insensitive) and stops early with a precise `StopReason::NoProgress`. Honest scope: it bounds wasted compute on a stuck model and sharpens the bench diagnostic — it does not lift a model's capability ceiling. *ADR-037.*

- **Sprint 28 — repeated-failure guard** (2026-06-27). Completed the loop-hardening **guard family**. The repetition guard (identical calls) and no-progress guard (same tool name) both key off the *actions* a model emits; neither catches a model emitting a *different* tool every turn that *all error* (bad paths, denials) and never recovers — both reset, so it grinds to `max_turns`. The new `FailureGuard` keys off tool *results*: consecutive all-errored turns → an early stop with `StopReason::RepeatedFailure`. Same honest scope — it bounds wasted compute and sharpens the diagnostic, not a model's capability ceiling. *ADR-038.*

- **Sprint 29 — `apply_patch` rounds out Ring 2** (2026-06-27). Pivoted from loop guards back to the **tool rings**. Added the second Ring-2 ("plan & apply structured changes") tool: `apply_patch` applies a context-located unified diff to one file, atomically. Unlike `multi_edit` (which only hits the *first* occurrence), a hunk's surrounding context **disambiguates** which occurrence to edit — and unified diffs are a format models emit naturally. Medium-tier models now offer 12 tools (Ring 0+1 + `multi_edit` + `apply_patch`). *ADR-039.*

- **Sprint 30 — Ornstein, increment 1** (2026-06-27). Began the **Animus** direction by hardening the loop's research story. Built **Ornstein**'s heart (recovered from the s1 roadmap) — a **quarantined summarizer** (`ferric-research`): untrusted content → a model with **no tools, no memory**, constrained to a **data-only** schema → a typed, provenance-tagged `ResearchDigest`. The quarantine is *structural* — it reuses Ferric's constrained valve, so a prompt-injection in the content can only ever surface as quoted **data**, never an action. See [`docs/ornstein.md`](docs/ornstein.md). *ADR-040.*

- **Sprint 31 — Ornstein increment 2** (2026-06-27). Ornstein is *one funnel, many sources*: the s30 quarantine is the universal sink; research is now multi-source. Added the keystone **`Retriever`** trait (`plane`/`available`/`retrieve`, async for the later network planes) + the first source — a **Local-FS retriever** — and the `research()` pipeline that runs a query **source → quarantine → provenance-tagged digest**. Even local files are untrusted (a downloaded doc, a NAS share), so every source routes through the funnel. Build order: Local FS ✅ → Tailnet/NAS → Web+container. See [`docs/ornstein.md`](docs/ornstein.md). *ADR-041.*

- **Sprint 32 — Ornstein increment 3** (2026-06-28). The second source plane: a **Tailnet/NAS-FS retriever** that searches a *remote* tailnet device's filesystem over **Tailscale SSH** (`tailscale ssh` for Linux boxes, plain `ssh -p` for Termux), feeding matches to the same quarantine. The security core — `ssh` runs its command through the *remote* shell, so the research query is single-quote-escaped against **remote command injection** — is fully unit-tested; the live SSH E2E is deferred (no tailnet SSH target reachable at build time). Build order: Local FS ✅ → Tailnet/NAS ✅ → Web. See [`docs/ornstein.md`](docs/ornstein.md). *ADR-042.*

- **Sprint 33 — Ornstein research orchestrator** (2026-06-28). The "one funnel, many sources" payoff: `research_all(planes, provider, query)` runs a query across every available source plane at once, quarantines each chunk, **dedups by source before the model call** (a file reachable from two planes costs one inference, not two), and returns the aggregated digests plus a per-plane outcome report (what ran / was offline / counts). Composes the local + tailnet planes with zero pipeline change. See [`docs/ornstein.md`](docs/ornstein.md). *ADR-043.*

- **Sprint 34 — CaMeL-lite sink-policy primitive** (2026-06-28). Co-designed with the user: flow control on top of the quarantine. `TaintSet` marks a digest's text as tainted; `SinkPolicy::decide(permission, tainted)` decides whether a `Write`/`Execute` tool call whose args derive from tainted data may proceed — **all three enforcement modes ship** (`Deny` / `RequireApproval` / `Warn`, caller's choice). Pure primitive only — an end-to-end test proves it would `Deny` an injected write once wired; the live gate lands at the dispatch chokepoint with the research→loop wiring. See [`docs/ornstein.md`](docs/ornstein.md). *ADR-044.*

- **Sprint 35 — expert review + refactor** (2026-06-29). The first full-project audit: what does Ferric need to become an operational, competitive, safe product, staying efficient for edge/personal-compute deployment? Full findings (file:line cited) in `sprints/s35/sprint-research/research-report.md`; four immediately-effective fixes shipped — a **read-side sensitive-file guard** (`.env`/SSH keys/cloud credentials can no longer be read into the plaintext trace), **`ferric server` edge-tuning flags** (`--threads`/`--gpu-layers`/`--batch-size` for Jetson/RPi-class latency tuning), **`mistralrs` rev-pinned** (was floating on `branch = "master"`), and **`reqwest` switched to `rustls-tls`** (no native OpenSSL dependency for ARM cross-compilation). Everything larger (MCP, a new chat mode, shell/git tools, streaming, session resume) is explicitly deferred with reasons, not silently dropped. *ADR-045.*

- **Sprint 36 — `ferric mcp`** (2026-07-03). The "ADR-005 security call" that blocked ADR-012 since sprint 1 is answered, and the MCP-stdio server it unblocks is built. `ferric mcp` exposes **exactly one** MCP tool, `ferric_query` (`{prompt, files?}`) — never Ferric's individual builtins and never workspace/backend/model as per-call parameters (those are launch-time-fixed CLI flags, so the containment guarantee is *structural*: a client can't redirect the workspace or swap the model because the wire protocol has no field for it). Every `tools/call` runs the same constrained agent loop `ferric query` drives, inheriting the guard/permission checks, tool rings, and per-call tracing. Hand-rolled JSON-RPC 2.0 (no `rmcp` dependency); the mistral.rs in-process hang was explicitly dropped rather than re-chased. *ADR-046.*

- **Sprint 37 — streaming inference** (2026-07-03). Fills ADR-003's reserved `complete_stream` extension point. `ConstrainedJson` returns every turn's completion — including the final answer — as one opaque JSON object, so raw token deltas aren't human-readable; a small incremental scanner (`ConstrainedJsonScanner`) extracts exactly two live signals as text streams in: an early "which tool is being called" activity signal, and — only for the `task_complete` answer — the live-decoded `summary` text, handling JSON escape sequences correctly across arbitrary chunk boundaries. `Provider::complete_streaming` ships with a default implementation (every non-overriding provider behaves identically to `complete()`, zero code change); `OpenAiProvider` gets a real SSE implementation via `Response::chunk()` (no new dependency). `ferric query --stream` is opt-in this increment; the constrained-decoding guarantee is unchanged — buffering is for display only, dispatch still runs on the fully-parsed completion. *ADR-047.*

- **Sprint 38 — persistent configuration + `Animus.md`** (2026-07-04). User-chosen: "persistent config and Animus.md (much like claude.md but for Animus)". `ferric query`/`ferric mcp` now resolve their tunables (backend, model, params_b/quant/family/ctx/temperature, max_ring, profile_dir, stream) as **CLI flag > project `.ferric/config.toml` > user config > hardcoded default** — a bounded, named field list (never a generic key-value map), so config can't touch security/guard/denylist policy. A plan-critic pass caught (and this sprint fixed, before shipping) a subtle masking-hazard class: a config-only-set value silently invisible to the code deciding behavior — twice, beyond the field it was actually reviewing (`BackendOpts.backend`'s own leftover clap default, and the ADR-029 profile-lookup key `model_key`, which had to be re-derived from the *post-merge* value). `Animus.md` — a project-root, freeform, user-authored instructions file — is read (no parsing) and folded into the system prompt as a distinct block; trusted context, not Ornstein-quarantined. *ADR-048.*

- **Sprint 39 — session resume** (2026-07-04). User-chosen, scoped down mid-research to resuming an **interrupted, still-incomplete task** (process crashed/killed mid-loop; continue the SAME task with more turns, no new prompt needed) — not a chat-continuation mechanism. Two new/extended trace events close the reconstruction gaps: `Event::SessionPrompt` (the original system+user text was never recorded before) and the terminator's `ToolCall` traced in every protocol (closes a pre-existing `NativeTools` audit gap where a session's summary was recorded nowhere at all). `ferric-loop::replay()` reconstructs the in-memory turn history from a trace file — a real design correction surfaced only during implementation: `TurnEnd` is written *before* dispatch, so a turn is only safely committed once a *later* `TurnStart` confirms its dispatch actually finished. `ferric query --resume <path>` replays and continues; a trace that already reached any stop reason is rejected (`AlreadyStopped`) — that gate is the real ADR-011 boundary. The user's own mid-research pivot reframed the backlog's `--save-interval` into **context-budget compaction** (`RunPolicy.prompt_budget_tokens` is computed and traced but never enforced) — big enough to become its own dedicated **sprint 40**, not bundled here. *ADR-049.*

- **Sprint 40 — context-budget compaction** (2026-07-04). Carved out of sprint 39's research when the user reframed `--save-interval`, unprompted, into a real gap: `RunPolicy.prompt_budget_tokens` was computed and traced but never enforced. New `HistoryCompactor` (`crates/ferric-loop/src/compact.rs`) is an always-on, no-CLI-flag mechanism (mirrors the repetition/no-progress/failure guards' precedent) that folds older turns into one model-summarized message once a session's `input_tokens` crosses 85% of budget, always preserving the most recent 2 turns verbatim. A foreground plan-critic pass caught the sprint's riskiest arithmetic before it shipped: an originally-planned `turn_offset` re-keying scheme was replaced with direct absolute-turn-number tracking (`Vec<(u32, usize)>`) in both the compactor and `replay()`'s reconstruction — simpler, and it closed a real gap where `replay.rs` was discovered to discard the turn number entirely. `replay()` was extended (a required fix, not optional) so a `--resume` of a compacted-then-killed session reconstructs the SHRUNK history, not the full pre-compaction one — proven by a real compact→kill→replay→resume round-trip test, mirroring sprint 39's own C-010 precedent. The summarizer reuses the SAME provider (no second, cheaper model exists in Ferric's one-local-model architecture); a failed summarization is non-fatal (logs a Note, skips the fold). *ADR-050.*

- **Sprint 41 — container architecture** (2026-07-09). User picked chat mode, then reframed it into a platform-wide question: should Animus run containerized (Docker/k8s/n8n), with chat/Ornstein/MCP in **nested** containers via Docker-in-Docker? Research found a correction: literal DinD is a security anti-pattern for isolating untrusted content specifically (2026 practice — incl. Docker's own **Docker Sandboxes**, GA Jan 2026 — uses **microVM-class sandboxes** for that use case), while ordinary **sibling containers** handle deployment flexibility (one machine → datacenter) — two different tools for two different problems the framing had conflated. The user chose "container architecture only" (chat mode → sprint 42). Shipped: **ADR-051** (the correction + the topology decision); a **`ferric-core`** multi-stage Dockerfile co-locating the `ferric` binary — built with `--features backend-openai` — and a prebuilt `llama-server`, so ADR-005's loopback pin between harness and backend is preserved by keeping them in ONE container (never split across a container boundary); a `docker-compose.yml` skeleton (`ferric-core` real, Ornstein/chat as marked stubs). **Docker Desktop was installed mid-sprint**, clearing the multi-sprint "no containerizer" blocker (had stalled Ornstein's web-retriever increment since sprint 30) and upgrading validation from design-only to a live `docker build` + `docker compose config`. *ADR-051.*

- **Sprint 42 — raw chat mode** (2026-07-09). The ADR-011-revision's second half (after `ferric mcp`), motivated by Animus IDE sending natural-language change requests conversationally. The user chose the **hybrid talk + escalate** shape. `ferric chat` is a REPL: **talk mode** (the default) is the harness's FIRST unconstrained-completion path — a single `provider.complete()` with no tools and no constraint, output text-only, printed and appended to history, **never dispatched** (the safety is structural: the talk path lives in the CLI and never touches `ferric-loop::run()`, which stays always-constrained). **`/do <request>`** escalates a turn into the existing constrained agentic loop (`run_with_provider` + `ferric-guard` + guards + trace, exactly as `ferric query`), USER-initiated only — the model can never self-escalate into acting (ADR-005). A foreground plan-critic caught two load-bearing issues pre-lock: a fixed mock script can't drive a stdin-length-driven REPL (→ a fresh per-turn mock), and `run()`'s per-call `SessionStart..SessionEnd` envelope can't be nested (→ each `/do` gets its own agentic trace file, mirroring `ferric mcp`). The structural-safety guarantee is proven both by a unit test (`talk_request` has empty tools + no constraint) and a black-box test (a talk line that *looks* like a tool call dispatches nothing). *ADR-052.*

- **Sprint 43 — Animus Launch increment 1** (2026-07-09). The first slice of a named Animus-suite pillar (the GECK successor): a project bootstrapper. Research found GECK (`~/GECK`) is Python-only and does macro-prompt scaffolding but **no git bootstrapping** — so Launch's distinct value is the deterministic, **LLM-free** "interview → real git repo (main+dev) + a sprint-loop-ready skeleton" flow. The user chose to build **both** the scaffolder and the interactive interview this increment. A new **`animus-launch`** library crate: `scaffold(&LaunchSpec)` refuses-to-clobber (never over a non-empty dir / non-dir path), creates the dir + a seed skeleton (README from goal, `.gitignore`, `agent-tasks/` with goal-derived tasks, `decisions.md`), and runs `git` as a checked closed-subcommand sequence (init → commit with a fixed identity so CI-without-identity works → `branch -M main` → `branch dev`). A **`ferric launch`** subcommand drives it: supplied flags skip the matching interview question, anything missing is asked on stdin (prompts to stderr so stdout stays the report). The key architectural point: Launch is user-run and LLM-free, so `ferric-guard`'s agent-containment doesn't apply — the sole safety property is refuse-to-clobber. A foreground plan-critic caught the nested-`agent-tasks/`-dir and clobber-semantics gaps pre-lock. *ADR-053.*

- **Sprint 45 — Ornstein Web Retriever (Inc 4)** (2026-07-13). Built the Web Retriever plane for Ornstein. Added a highly restricted microVM-class Docker sandbox (`crates/ferric-research/src/sandbox.rs`) that drops all capabilities and routes traffic through an allowlist proxy (`docker/squid.conf`). Implemented `WebRetriever` using `wget` to safely fetch URLs into the quarantine, ensuring all untrusted external content remains isolated from Ferric's execution environment. *ADR-055.*

- **Sprint 46 — Loop Research-Phase Wiring & Sink Policy (Inc 5)** (2026-07-13). The final CaMeL wiring. Moved `sink.rs` to `ferric-guard` to form a true dependency tree (`ferric-tools` → `ferric-guard` → `ferric-core`). Wired `TaintSet` and `SinkPolicy` all the way down through `ferric-cli`'s `--research` and `--sink-action` flags into `registry.execute`. A tool call attempting a Write or Execute sink with tainted arguments is now intercepted and evaluated according to the chosen sink policy (`deny`, `warn`, `requireapproval`). *ADR-056.*

- **Sprint 47 — Testing System** (2026-07-13). Built the 4-tier testing system to provide regression safety for the fast-moving loop. Increment 1: **Golden Trace Testing** (`ferric trace verify`) reconstructs the `MockProvider` and checks a deterministic trace for event mismatches. Increment 2: **Containerized E2E Harness** (`tools/run-e2e.sh`) launches `ferric-core` inside Docker to execute a mock query. Increment 3: **Coverage Scripts** (`tools/run-coverage.sh`) leverages `tarpaulin` to enforce the 80% coverage threshold. *ADR-057.*

- **Sprint 48 — Animus Launch Increment 2** (2026-07-13). Extended `ferric launch` to natively support generating project skeletons based on an explicit project type (`Rust`, `Python`, `Web`, or `Empty`). After scaffolding the repository, it now features an auto-hand-off mechanism that prompts the user if they'd like to immediately begin execution with the Ferric agent using their goal as the prompt, eliminating the friction between bootstrapping a repo and starting the Sprint 1 loop. *ADR-058.*

- **Sprint 49 — Streamlining Streaming & Usability Assessment** (2026-07-13). Delivered `talk-mode` streaming for `ferric chat` and `ferric/streamDelta` JSON-RPC notifications for `ferric mcp` to solve laggy IDE UI feedback. Assessed project usability, concluding that it is functionally usable, natively LLM-free, and contained, but requires container operationalization and compaction tuning next. *ADR-059.*

- **Sprint 50 — Native Tailscale Integration** (2026-07-13). Added a `--tailscale` flag to `ferric server up`. Ferric now natively orchestrates Zero-Trust WireGuard tunneling by shelling out to `tailscale serve <port>` immediately after the engine (llama-server/Ollama) binds to loopback. This allows secure, authenticated, off-device access to the local inference API without exposing public firewall ports. *ADR-060.*

- **Sprint 51 — Skill Loader via MCP Prompts** (2026-07-13). Added dynamic Knowledge Skill loading. `ferric mcp` now scans `.ferric/skills/*/SKILL.md` and serves them via the MCP `prompts/list` and `prompts/get` protocols. Agents connected to Ferric (Cursor, Claude Code, etc.) can now discover and inject project-specific skills dynamically into their context. *ADR-061.*

- **Sprint 52 — Advanced File Tooling & ICM Adaptation** (2026-07-14). Adapted the file tooling to better support the Interpretable Context Methodology (ICM) across all tiers. Promoted `copy_file`, `move_path`, and `search_files` to Ring 0 (core) so Nano models can navigate workspace hierarchies, proportionally scaling up tier tool caps (e.g., Nano: 10, Small: 14) to accommodate. Implemented `start_line` and `end_line` pagination in `read_file` to allow small-context models to read large files safely. Validated with comprehensive tests across `ferric-core`, `ferric-cli`, and `ferric-tools`. *ADR-062.*

- **Sprint 54 — Ornstein Validation & IDE Integrations** (2026-07-14). Resolved `cargo clippy` warnings in `ferric-cli` and `ferric-loop` to pass CI. Validated the Ornstein `WebRetriever` using `--mock` to bypass local environment restrictions. Cloned and rebranded `Animus_IDE` to point to OpenVSX and stripped Microsoft telemetry to set up a clean, agentic-ready IDE fork.

- **Sprint 55 — `ferric mcp --resume` support** (2026-07-14). Added the `--resume <path>` flag to `ferric mcp` to allow resuming an interrupted task session over MCP. The resume state is verified, stored, and passed down directly into the first `ferric_query` `tools/call`.

- **Sprint 56:** Per-tier compaction tuning. `HistoryCompactor` triggers now scale natively per tier in `RunPolicy`.

- **Sprint 57 — Chat Mode Refinements** (2026-07-14). Integrated `rustyline` into `ferric chat` for robust REPL history and line editing, improving interactive workflow for conversation-driven escalations.

- **Sprint 59 — `shell_exec` tool & CaMeL wiring** (2026-07-15). Delivered the `shell_exec` tool (Ring 2) and addressed ADR-045's deferred requirements. Extended the `Tool` trait with `target_commands()` and added `ferric-guard::check_command` to screen commands against static denylists prior to child process execution. Wired the CaMeL sink policy `TaintSet` into the `Registry::execute` chokepoint to govern `Write` and `Execute` tool calls according to the user's `--sink-action` flag.

- **Sprint 62 — Schema-Enforced Chain of Thought** (2026-07-16). Replaced the nested JSON schema representation for `action` with a flattened, highly stable wrapper that explicitly forces the `thought` field *before* tool parameters. Fixed a critical agent hallucination where the missing JSON schema `description` metadata (stripped by `llama-server`) caused the LLM to misuse `make_dir` repeatedly for file creation; explicitly injected `registry_tools` descriptions into the system prompt for constrained environments.

> **Next — TBD.** Open: operationalize the `ferric-core` container (run it end-to-end with a mounted model) + multi-arch images; wiring chat into the Animus IDE.

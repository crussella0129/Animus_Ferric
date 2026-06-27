# Sprint 23 Research Report — llama.cpp (llama-server) as the first-class engine

> Goal (user): replace the ollama path with full llama.cpp — for higher tok/s, a
> user-extendable context window (better agentic results), a route to the multimodal
> engine (mmproj), and edge-class minimalism (Jetson Orin Nano / Pi + AI hat).

## Surprise finding: Ferric already *prefers* llama-server
`crates/ferric-cli/src/server.rs`: `--engine` defaults to **`llama-server`** (ollama
is the *pluggable* alternative). `command()` already emits
`llama-server -m <gguf> [--mmproj <gguf>] -c <ctx> --host 127.0.0.1 --port <p>` —
so **context size (`-c`) and multimodal (`--mmproj`) are already wired**. The whole
backend is engine-agnostic via the OpenAI-compatible valve (`--backend openai
--api-base`). So this is **not** new architecture — it's *validation + making
llama.cpp the path we actually use/document*. Every bench to date used ollama only
because that's what's installed.

## Why llama.cpp over ollama (the user's study, grounded)
- **tok/s:** llama-server is the raw engine; ollama wraps it with model-management overhead + conservative defaults.
- **context:** ollama defaults context narrow (num_ctx 2048/4096 unless tuned per-model); llama-server's `-c` goes as wide as VRAM allows (`-c 0` = the model's full trained context) — directly the user's "extend as wide as possible" for agentic runs.
- **multimodal:** llama-server + `--mmproj` is the documented vision/audio path (Ferric already exposes `--mmproj`); ollama's multimodal is model-gated and narrower.
- **edge minimalism:** llama.cpp is *the* edge engine — a single static binary, CPU + CUDA/Vulkan/Metal builds, runs on Jetson/Pi. ollama is a heavier Go daemon + registry. For "minimal + efficient enough for a Pi," llama.cpp wins decisively.

## The blocker (and the trick around it)
- **llama-server is NOT installed here**, and there are **no standalone GGUF files** (only ollama).
- **But ollama's model blobs (`~/.ollama/models/blobs/sha256-*`) ARE GGUF files** — the largest blob per model is the weights. So a llama-server can be pointed at an existing blob with **no multi-GB re-download**.
- A prebuilt llama.cpp **release** (Windows zip from the official repo, CPU build) is a ~50–150 MB fetch — feasible for a live spike.

## Decisions Reviewed
- **ADR-001 / ADR-005** — the constrained path is the OpenAI HTTP valve, bound to loopback. llama-server *is* that valve (it enforces GBNF/json_schema). Nothing changes here.
- **ADR-026/027** — multimodal is gated; llama-server + mmproj is the intended media engine. This sprint makes that path first-class.

## Recommended approach (scope)
**Make llama.cpp first-class + validate it, with the live run attempted not assumed:**
1. **Launcher contract (AI-verifiable):** unit-test `server::command()` for the llama-server argv (`-m/-c/--host/--port`, mmproj when set); confirm `-c` widens context.
2. **ADR-032:** llama.cpp as the primary engine — rationale (tok/s, wide context, multimodal, edge); ollama stays a pluggable fallback; the loopback/valve invariants unchanged.
3. **Docs:** a llama-server-first quickstart (install via prebuilt release; **point `-m` at an ollama blob** to skip re-download; `-c 0` for max context) + Jetson/Pi edge notes; reframe README/run_benchmarks to lead with llama-server.
4. **Live validation ATTEMPT (the headline, ExitPlanMode-gated):** fetch a prebuilt `llama-server`, run it on an ollama GGUF blob, point `ferric --backend openai --api-base http://localhost:8080/v1` at it, run `query` + a short `bench`/`toolbench`, and compare tok/s + context to ollama. **If the fetch/run fails** (CUDA/DLL/network), the launcher tests + ADR + docs still land and the live A/B is deferred to the user with exact steps (human-gated on install).

## Risk
- **Install may fail autonomously** (prebuilt binary DLLs, GPU build mismatch). Mitigation: the sprint's value doesn't depend on it — the contract + ADR + docs are the floor; the live run is upside. Recorded honestly either way.
- **PR cadence:** PR #8 (sprint 22) is still open on `dev`; a clean sprint-23 PR needs #8 merged first (else it bundles) — flag to the user ([[one-pr-per-sprint]]).

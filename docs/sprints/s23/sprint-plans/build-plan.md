Finalized - DO NOT EDIT

# Sprint 23 Build Plan — llama.cpp (llama-server) first-class + live A/B

Ferric already defaults to `llama-server` (mmproj + `-c` context wired) but it's
never been run (only ollama installed). Validate + make-first-class the preferred
engine, with a live A/B vs ollama (point `llama-server -m` at an ollama GGUF blob to
skip the download). Rationale: `sprints/s23/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** llama.cpp is the validated, documented primary engine.
  - **A. launcher contract** — T-2301
  - **B. ADR-032 + docs** — T-2302
  - **C. live install + A/B** — T-2303 (test phase)

## Execution Sequence

### T-2301: Lock the llama-server launcher contract
- **Touches:** `crates/ferric-cli/src/server.rs` (tests)
- **Success (EARS):** unit test on `command()` for `Engine::LlamaServer` → `-m`, `-c <ctx>`, `--host 127.0.0.1`, `--port`, `--mmproj` iff set; ollama → `serve`.

### T-2302: ADR-032 + llama-server-first docs
- **Touches:** `decisions.md`, `README.md`, `docs/llama-cpp.md` (new), `run_benchmarks.ps1`
- **Success (EARS):** ADR-032 (llama.cpp primary; tok/s/wide-context/multimodal/edge; ollama fallback; invariants unchanged). Docs: llama-server quickstart (prebuilt install, ollama-blob trick, `-c` wide context, `--mmproj`) + Jetson/Pi edge notes; README leads with llama-server.

### T-2303: live install + A/B (test phase, headline)
- Fetch prebuilt llama.cpp (CPU win release) → run on an ollama GGUF blob → `ferric --backend openai --api-base :8080/v1 --protocol grammar` query/toolbench → confirm constrained decoding works on llama-server; A/B tok/s + wide `-c` vs ollama. Fallback: defer the live A/B with exact steps if the binary won't run.

## Post-build (test)
- launcher test + workspace green; the live llama-server validation + A/B.

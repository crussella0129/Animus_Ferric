Finalized - DO NOT EDIT

# Sprint 23 Test Plan — llama.cpp first-class + live A/B

## Unit (`ferric-cli`, default CI)
- `server::command()` for `Engine::LlamaServer` → argv contains `-m <model>`, `-c <ctx>`, `--host 127.0.0.1`, `--port <p>`, and `--mmproj <p>` iff `mmproj` set; `Engine::Ollama` → `serve`.

## Build / Lint
- `cargo test --workspace` green; `clippy --workspace --all-targets -- -D warnings` clean; `fmt --check` clean.

## End-to-End — RUN it (the headline: first llama.cpp validation)
1. Fetch a prebuilt llama.cpp Windows CPU release → scratch dir; unzip → `llama-server.exe`.
2. Resolve an ollama GGUF blob for `llama3.2:1b` (manifest → `image.model` digest → `blobs/sha256-<digest>`).
3. `llama-server -m <blob> -c 8192 --host 127.0.0.1 --port 8080` (background); poll `/health`.
4. `ferric query --backend openai --api-base http://localhost:8080/v1 --model llama3.2-1b --protocol grammar --workspace <tmp> "create hello.txt with hi"` → **confirm constrained decoding drives the loop on llama-server** (task completes, file created) — the valve thesis on the new engine.
5. **A/B vs ollama:** a short `toolbench`/`query` on both; compare tok/s (trace `TurnEnd` tokens ÷ wall) and try a **wide context** (`-c 16384`) the way ollama can't by default.
6. Record numbers + the verdict (does Ferric drive llama-server? does it match/beat ollama? does wide context work?) in the test-report.

## Fallback (honest)
- If the prebuilt binary won't run (DLL/CUDA/network), the launcher test + ADR-032 + docs still land; the live A/B is **deferred to the user** with the exact install + run commands (incl. the ollama-blob trick). The sprint's floor doesn't depend on the live run.

Finalized - DO NOT EDIT

# Sprint 25 Test Plan — Validate Gemma 4 E4B

## Build / Lint (default CI)
- No Ferric code change expected (validation sprint). `cargo test --workspace` green; `clippy --workspace --all-targets -- -D warnings`; `fmt --check`.

## End-to-End — RUN it (the headline)
**Setup:** download `google/gemma-4-E4B-it-qat-q4_0-gguf` (model 5.15GB + mmproj 0.99GB); serve with `llama-server -m model.gguf --mmproj mmproj.gguf -c 8192 --port 8080` (update llama.cpp build first if `b9821` doesn't know the Gemma 4 arch); `/health` ok.

1. **Agentic floor — `ferric bench`:** L0–L6 via `--backend openai --api-base :8080/v1 --model gemma-4-e4b --params-b 4 --protocol grammar`. Record per-level PASS/FAIL + `measured_level`. **Question:** does ~4B clear levels the 1B (none) couldn't? Where does it land vs 7B (6) / 8B (5)? Any value confirms/refines the floor.
2. **Multimodal under the constraint:** `ferric query --file red96.png --modality image --protocol grammar "describe the image in one sentence, then call task_complete with that description"`. **Question:** does a *capable* model describe the image **inside the agentic loop** (where SmolVLM-500M garbled)? A correct red/colour reference in `task_complete` ⇒ ADR-033 closed without a harness change.
3. **Constrained valve fire-rate:** `ferric toolbench --api-base :8080/v1 --protocol grammar --max-ring 0 --iterations 6` → Ring-0 % on Gemma 4 (expect solid, like the fleet).

## Fallback (honest)
- If Gemma 4 won't load even on a fresh llama.cpp build, the research verdict (usable: official GGUF + mmproj, 4B + function-calling + multimodal) + the download path + ADR-035 still land; the live numbers defer to a working build (the *only* gap is the runtime).

## Notes
- All three are AI-verifiable (PASS/FAIL, measured_level, fire-rate, a colour reference). The agentic + multimodal results are the sprint's evidence that ~4B is the usable floor for Ferric.

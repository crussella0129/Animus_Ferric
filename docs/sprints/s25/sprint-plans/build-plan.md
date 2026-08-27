Finalized - DO NOT EDIT

# Sprint 25 Build Plan — Validate Gemma 4 E4B (Ferric's reference ~4B multimodal model)

The ~4B agentic floor (1B none / 7B 6 / 8B 5; sub-1B VLMs garble, ADR-031/033) means
the answer isn't a `--chat` workaround — it's a capable small model. **Gemma 4 E4B**
(official llama.cpp GGUF + mmproj, 4B + function-calling + vision/audio + 128K) is the
fit. Validate it agentic + multimodal on llama.cpp. Rationale:
`sprints/s25/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** prove ~4B Gemma 4 E4B is the usable agentic + multimodal floor.
  - **A. download + serve** — T-2501
  - **B. validate agentic + multimodal** — T-2502
  - **C. ADR-035 + docs** — T-2503

## Execution Sequence

### T-2501: Download + serve Gemma 4 E4B
- Fetch official `google/gemma-4-E4B-it-qat-q4_0-gguf` (model 5.15GB + mmproj 0.99GB).
- Confirm prebuilt `b9821` loads the arch + mmproj; else fetch a current llama.cpp release.
- `llama-server -m model --mmproj mmproj -c 8192 --port 8080`; `/health` ok.

### T-2502: Validate agentic + multimodal (headline)
- **Agentic:** `ferric bench --backend openai --api-base :8080/v1 --model gemma-4-e4b --params-b 4 --protocol grammar` L0–L6 → `measured_level` vs 1B none / 7B 6 / 8B 5.
- **Multimodal under constraint:** `ferric query --file red.png --modality image --protocol grammar "describe … then task_complete"` → capable model describes inside the loop (closes ADR-033).
- Ring-0 toolbench fire-rate for the constrained-valve number.

### T-2503: ADR-035 + docs
- **Touches:** `decisions.md`, `docs/llama-cpp.md`, `README.md`, `run_benchmarks.ps1`
- ADR-035 (Gemma 4 E4B = recommended reference model; ~4B floor; sub-4B fails). Docs: Gemma 4 E4B quickstart; README Status 25 + Sprint 25 timeline. `--chat` dropped.

## Post-build (test)
- workspace green (no Ferric code change expected) + the live Gemma 4 E4B bench + multimodal + toolbench.

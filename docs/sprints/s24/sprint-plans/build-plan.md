Finalized - DO NOT EDIT

# Sprint 24 Build Plan — Live multimodal heartbeat (llama-server + mmproj)

Finally run an image end-to-end through Ferric's multimodal pipeline (built s10,
never run live) against a real vision model on llama-server (`--mmproj`). No Ferric
code change expected — the send path is already built + tested. Rationale:
`sprints/s24/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** prove image → Ferric → llama-server(mmproj) → a seeing model.
  - **A. live heartbeat** — T-2401
  - **B. ADR-033 + docs** — T-2402

## Execution Sequence

### T-2401: Live multimodal heartbeat
- Fetch a small vision GGUF + mmproj (SmolVLM-500M; fallback moondream2) → scratch dir.
- `llama-server -m <vlm> --mmproj <mmproj> -c 4096 --host 127.0.0.1 --port 8080`.
- Generate a tiny known test image (solid-color square).
- `ferric query --backend openai --api-base :8080/v1 --file test.png --modality image "describe the image … then task_complete"` (grammar; try native if the constraint fights the VLM).
- **Success:** the trace shows the image went out as a content-part + the response references it. Small Ferric fix only if the live run exposes one.

### T-2402: ADR-033 + docs
- **Touches:** `decisions.md`, `docs/llama-cpp.md`, `docs/multimodal.md`, `README.md`
- **Success:** ADR-033 (multimodal validated E2E, model + result); `--mmproj` heartbeat walkthrough; README Status 24 + Sprint 24 timeline.

## Post-build (test)
- workspace green (multimodal units already cover the mapping) + the live heartbeat.

## Loop close (one PR per sprint)
- commit → push `dev` (visible, no `-q`) → verify `origin/main..dev` = s24 only → sprint-24 PR (verify count) → schedule next.

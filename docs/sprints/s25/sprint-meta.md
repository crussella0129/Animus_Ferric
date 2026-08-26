# Sprint 25 Meta

- **Sprint number:** 25
- **Start timestamp:** 2026-06-27T02:47:22Z
- **End timestamp:** 2026-06-27T03:30:00Z
- **Model:** claude-opus-4-8
- **Exit status:** success
- **Token count:** (not observed)
- **Summary:** Validated Gemma 4 E4B as Ferric's reference ~4B multimodal model (ADR-035; research pivot from a `--chat` workaround per the user — a capable model is the right answer to the ~4B agentic floor). Downloaded the official ungated GGUF + mmproj; the existing b9821 llama-server loaded it (no update). Results: agentic L0–L6 → measured_level 5 (matches the 8B, above the 1B's none → confirms ~4B is the usable agentic floor; L0/L2/L6 fails mostly CPU-speed timeouts); multimodal INSIDE the constrained loop → task_complete("a solid red rectangle"), closing ADR-033 with no harness change; Ring-0 toolbench 100% solid. No Ferric code change. Caveat: use a GPU llama.cpp build for speed.

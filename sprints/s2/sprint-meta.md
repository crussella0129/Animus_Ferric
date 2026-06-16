# Sprint 2 Meta

- **Sprint number:** 2
- **Start timestamp:** 2026-06-12T00:42:15Z
- **End timestamp:** 2026-06-15T20:00:00Z
- **Model:** claude-fable-5 (build) / claude-opus-4-8 (test phase)
- **Exit status:** failed
- **Token count:** (not observed)
- **Summary:** FAILED on the headline goal — the unified action grammar hangs the real mistral.rs/GGUF engine (root-caused; tokenizer.json fix disproven) and the 1B can't cleanly terminate in native mode. All OTHER deliverables shipped + model-free-verified (112 tests, CI green): ferric-prompt (oovra), the grammar machinery, ferric-bench L0–L6 + calibration, move_path/make_dir, output budgets, ADR-015..020. s3 begins from failure-report.md: inference-timeout safety net, grammar-enablement fork, and a tool-tuned capability model (Gemma 4 12B / Qwen-7B).

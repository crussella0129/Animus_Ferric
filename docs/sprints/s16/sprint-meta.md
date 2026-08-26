# Sprint 16 Meta

- **Sprint number:** 16
- **Start timestamp:** 2026-06-25T02:01:04Z
- **End timestamp:** 2026-06-25T02:45:00Z
- **Model:** claude-opus-4-8
- **Exit status:** success
- **Token count:** (not observed)
- **Summary:** Shipped ring calibration — `ferric toolbench --calibrate-rings` sweeps a model ring-by-ring and reports the highest ring it reliably drives (the recommended `--max-ring`). Pure `recommend_max_ring` (highest unbroken solid-prefix ring) + a calibrate branch reusing `bench_model`/`verdict` that auto-stops when a ring adds no tools. Proven vs ollama: qwen2.5-coder:7b AND llama3.2:1b both calibrate to `--max-ring 1` at 100%. Closes the rings loop — a model earns a wider grammar by proving it on the bench.

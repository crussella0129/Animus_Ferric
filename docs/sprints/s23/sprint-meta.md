# Sprint 23 Meta

- **Sprint number:** 23
- **Start timestamp:** 2026-06-27T01:26:11Z
- **End timestamp:** 2026-06-27T02:30:00Z
- **Model:** claude-opus-4-8
- **Exit status:** success
- **Token count:** (not observed)
- **Summary:** Validated Ferric on full llama.cpp (llama-server) for the first time and made it the first-class engine (ADR-032). Ferric already defaulted to llama-server (launcher correct + unit-tested) but it had only ever run against ollama. Live: fetched the prebuilt b9821 CPU release, pointed -m at an ollama GGUF blob (no re-download), and drove the constrained loop → created a file + a Ring-0 toolbench scored 36/36 = 100% solid, identical to ollama. So the OpenAI constrained valve is engine-agnostic and works on full llama.cpp, with wide context (-c), multimodal (--mmproj), and a single edge-ready binary (Jetson/Pi). ollama stays a one-flag fallback. New docs/llama-cpp.md; README leads with llama-server. No launcher code change needed.

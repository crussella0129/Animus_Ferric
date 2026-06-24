# Sprint 9 Research Report — Determining Next Steps from the Sprint-8 Test

> Per the directive: this research is pointed at the real-model test findings
> (`sprints/s8/sprint-tests/findings-report.md`, ADR-024) to decide what to build
> next. The finding in one line: **the constrained-decoding thesis is empirically
> validated (100% constrained vs 0% native on a real model), the testbench works
> end-to-end, and the foundation is solid.** So the question is genuinely "build
> up (new capability) or cash in (use the testbench)?" — a real fork.

## Decisions Reviewed
- **ADR-024 (s8 test)** — the validating findings; constrained works, native is unreliable through ollama (returns tool calls as text), the testbench + launcher work end-to-end. **Primary input.**
- **ADR-023 (s7→8)** — pre-committed **multimodal "any file" input** as sprint 9, with the design (Message content-parts + capability gating + OpenAI media mapping). The strong prior.
- **ADR-019 (s2)** — `ferric bench` owns `measured_level` (the L0–L6 ladder). **Fleet calibration** would feed this; the toolbench stays a separate human-facing readout (keep distinct).
- **ADR-003/ADR-022** — `Message` is the type multimodal must extend (content parts); capability honesty (a `modalities` set extends `supports_constraint`'s principle).
- **ADR-001** — the HTTP valve is the multimodal path (llama-server/ollama via libmtmd).

No prior decision is violated; this sprint either *executes* ADR-023 or *exploits* ADR-024 — see the recommendation.

## 1. Sprint Goal (candidate)
Decide and execute the highest-value next step now that the constrained tool-calling foundation is proven. Two strong candidates, plus two small riders.

## 2. Existing Code Survey
| File | Relevance | Notes |
|------|-----------|-------|
| crates/ferric-core/src/message.rs | high | `Message { text: Option<String>, … }`. Multimodal must make content a list of parts (text \| media). The cross-cutting change. |
| crates/ferric-provider/src/openai.rs | high | `map_message` builds the OpenAI content; multimodal maps media parts to `image_url`/`input_audio`. |
| crates/ferric-core/src/scale.rs | high | `Capabilities`/`ModelProfile`; multimodal adds a `modalities` set (capability gating, ADR-022 principle). |
| crates/ferric-tools/src/builtin/read_file.rs | med | Text/code files already flow as text here; media needs a new routing path (a tool or content layer). |
| crates/ferric-cli/src/toolbench_cmd.rs | high | The validated diagnostic; **fleet calibration** = drive it across `D:\Models` and tabulate. Already produces JSONL. |
| crates/ferric-cli/src/bench_cmd.rs | med | The L0–L6 `bench` (the `measured_level` producer, ADR-019); calibration could feed it or stay a toolbench sweep. |
| crates/ferric-cli/src/server.rs | med | The launcher; multimodal needs `--mmproj` (already a flag) + a multimodal-capable engine. |
| sprints/s8/sprint-tests/findings-report.md | high | The determining input — foundation validated; testbench works; multimodal models sit in `D:\Models`. |

## 3. External Sources
- [Gemma 3n overview](https://ai.google.dev/gemma/docs/gemma-3n) — the user's `gemma-4-e4b` = Gemma 3n E4B, native image/audio/video (the multimodal target).
- [llama.cpp multimodal.md](https://github.com/ggml-org/llama.cpp/blob/master/docs/multimodal.md) — `llama-server -m … --mmproj …`; image/audio/video over `/chat/completions`.

## 4. Risks, Unknowns, Dependencies
- **Multimodal testability gap (decisive).** The user's multimodal models (Gemma 3n, Qwen3-VL) are **GGUFs in `D:\Models`, not in ollama**, and **`llama-server` is not installed**. So a multimodal **E2E acceptance** needs either (a) `ollama pull` a vision model, or (b) install `llama-server` + use `--mmproj`. The code + unit tests can land without that, but the heartbeat can't run until a multimodal server exists. (Image is well-supported; **audio/video** support in ollama specifically is the bigger unknown — llama-server's libmtmd is the surer path.)
- **Fleet calibration is immediately testable** with what's installed: ollama (qwen2.5-coder:7b, llama3.1:8b) + the GGUFs via the mistral.rs in-process path. Zero new setup. It directly cashes in sprint 8.
- **Multimodal `Message` refactor is cross-cutting** — touches core, every provider's `map_message`, the loop, traces. Bigger blast radius than calibration.
- **mistral.rs 0.8.15 viability** (ADR-023 gate) and the **native-`content` fallback** (ADR-024) are both small, independent riders.

## 5. Recommended Approach
**The data points two ways, and the choice is a real product call — so this research determines the *options* and a recommendation, and asks for the steer before locking a plan.**

- **Option A — Multimodal "any file" input (execute ADR-023).** The strategic capability the user explicitly wants ("process any file… audio/video"). Foundation is ready. *Cost:* a cross-cutting `Message` refactor; *and its E2E acceptance is blocked on a multimodal server the machine doesn't yet have* (ollama vision pull or llama-server install).
- **Option B — Fleet calibration (exploit ADR-024).** Turn the validated testbench loose across `D:\Models` (0.5B → 14B) + ollama, producing the real "which model is good enough" capability table — the literal purpose Ferric states ("small models, large alike; see what works acceptably"). *Fully testable now, zero setup, small surface.* Lower ceiling than multimodal but immediate payoff.
- **Riders (small, fit in either):** the mistral.rs 0.8.15 grammar-hang probe (decide its future, ADR-023/024); a native-`content` fallback (scrape a tool-call-shaped `content` when `tool_calls` is null — the ADR-024 ollama quirk).

**Recommendation:** **Option B (fleet calibration) first, then Option A (multimodal) as sprint 10.** Rationale: the test we just ran is *itself* the argument — the testbench works and the models are sitting right there; calibrating the fleet is the immediate, fully-verifiable payoff and produces lasting value (the capability table), whereas multimodal's acceptance is gated on setup the user must do first. Bundle the mistral.rs viability probe into calibration (it's a model in the fleet). **But this is a genuine fork the user should confirm** — if multimodal is the priority regardless of the testing-setup cost, Option A is equally valid and ADR-023 already blessed it.

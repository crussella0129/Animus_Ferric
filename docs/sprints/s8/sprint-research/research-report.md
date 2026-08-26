# Sprint 8 Research Report

> Scope: **the self-diagnostic testbench for the new (post-s7) architecture.**
> Two user-emphasized deliverables: (1) a *fully self-diagnostic* toolbench that
> runs each tool many times, classifies *why* failures happen, and writes a
> report so a user can dial a model down until quality drops; (2) a `ferric
> server` launcher so the constrained HTTP valve is one command. Plus a flagged
> mistral.rs viability experiment. **Multimodal "any file" input is deferred to
> sprint 9** — its design is already locked in ADR-023; doing it here would
> dilute both efforts (it's a cross-cutting `Message`-content-parts +
> capability-gating change deserving its own sprint).

## Decisions Reviewed
- **2026-06-10 ADR-001 — dual backend, HTTP valve** (s0) — relevance: the launcher manages the valve's server lifecycle; reaffirmed.
- **2026-06-11 ADR-011 — command structure, `ferric serve` is an additive subcommand** (s1) — relevance: the launcher lands in the reserved `ferric serve`/`server` slot; no chat catch-all is added.
- **2026-06-13 ADR-019 — `ferric bench` is the sole producer of measured_level** (s2) — relevance: the **toolbench is distinct** from `bench` (single-turn per-tool fire rate vs the L0–L6 trace-verified ladder). The diagnostic toolbench does NOT write `measured_level`; it's a human-facing "is this model good enough" report. No conflict; explicitly kept separate.
- **2026-06-23 ADR-022 — honest capabilities + protocol trichotomy** (s7) — relevance: the toolbench selects its protocol from the backend's real `capabilities()` via `select_protocol`, so it measures the path `ferric query` actually runs.
- **2026-06-23 ADR-023 — HTTP valve is the workhorse; `ferric server` launcher (llama-server default, Ollama pluggable); multimodal direction; functionality-over-purity** (s7→8) — relevance: **this sprint realizes the launcher + diagnostic-testbench halves of ADR-023.** Multimodal is the deferred half.
- **2026-06-10 ADR-005 — security hardcoded, harness-owned** (s0) — relevance: the launcher spawns an external server; it MUST bind `127.0.0.1` only, never `0.0.0.0`, and must not become a way to execute arbitrary binaries. The LLM is never consulted on the server command.

No prior decision is violated; this sprint *implements* ADR-023.

## 1. Sprint Goal
Build the self-diagnostic testbench. (a) **Enhance `ferric toolbench`** from a fire-rate printer into a diagnostic instrument: per-tool stats with a *failure taxonomy* (no-action / wrong-tool / malformed-args / parse-error / provider-error), an overall verdict that answers "is this model good enough for agentic use," and a written report (Markdown + machine-readable JSONL) — so a user can run progressively smaller models and watch where it breaks. (b) **Add `ferric server`** — a launcher that brings up the OpenAI-compatible server (default `llama-server`, Ollama pluggable via `--engine`), waits for health, writes a runfile (`pid`+`port`+`base_url`) that `query`/`toolbench` auto-discover, and `doctor`-checks first-run setup. Together these are the one-command "spin it up and see if your model can drive the tools" loop. (c) **Flagged experiment:** re-run the constrained-decoding hang probe against the bumped mistralrs **0.8.15** to decide mistral.rs's fate (ADR-020/023).

## 2. Existing Code Survey
| File | Relevance | Notes |
|------|-----------|-------|
| crates/ferric-cli/src/toolbench_cmd.rs | high | The s7 (T-007) base. Has `extract_action(protocol, completion) -> Option<ToolCall>` + `build_request` + per-tool fire-rate loop. Enhance: richer outcome (not just Option), failure taxonomy, report writer. |
| crates/ferric-cli/src/main.rs | high | `Command` enum (`Query/Bench/Toolbench/Trace`). Add `Server { up/down/status/doctor }`. |
| crates/ferric-cli/src/backend.rs | high | `BackendOpts`/`BackendArg{Mistral,Openai}` + `create_provider`. Toolbench reuses; launcher writes the base_url the OpenAI backend reads. |
| crates/ferric-cli/src/query.rs | medium | `ProtocolArg`→`ActionProtocol`, `select_protocol(policy, caps, override)`. Toolbench mirrors this; both can read the server runfile for `--api-base` default. |
| crates/ferric-provider/src/openai.rs | high | `OpenAiConfig{ base_url, api_key, model }`. Server discovery: the launcher's runfile feeds `base_url`. `/health` + `/v1/models` are the readiness signals. |
| crates/ferric-loop/src/grammar.rs | high | `action_schema(tools)` + `parse_json_action`/`parse_action`. The diagnosis validates parsed args against each tool's `input_schema` to distinguish malformed-args from wrong-tool. |
| crates/ferric-core/src/scale.rs | medium | `ActionProtocol`, `ModelProfile`, `Capabilities`. Toolbench builds a profile; verdict thresholds may key off tier. |
| crates/ferric-tools/src/registry.rs | medium | `tools_for_policy` — the tool set the toolbench enumerates (already used). |
| crates/ferric-cli/src/bench_cmd.rs | medium | The L0–L6 `bench` (spawn-self runner, results.jsonl). Pattern reference for the toolbench report writer + a reminder to keep the two commands distinct (ADR-019). |
| sprints/s6/sprint-tests/toolbench-results.md | high | **Prior-art report format** the user referenced ("ran each tool N times, wrote a report, diagnosed failures"). The new writer reproduces this shape, generated, with the failure taxonomy added. |
| run_benchmarks.ps1 / test_both_models.ps1 | medium | Current drivers that manually start a server / GGUF. `ferric server up` replaces the manual server step; these become thin wrappers. |
| crates/ferric-provider/src/mistralrs.rs | low | The viability experiment re-runs `grammar_probe` against this under 0.8.15 (the dep already resolves 0.8.15, confirmed in s7 testing). |

## 3. External Sources
- [llama.cpp multimodal.md (ggml-org)](https://github.com/ggml-org/llama.cpp/blob/master/docs/multimodal.md) — `llama-server` launch (`-m model.gguf --mmproj proj.gguf -c <ctx>`), OpenAI-compatible `/chat/completions`, libmtmd image/audio/video. Grounds the launcher's llama-server command (and sprint 9's multimodal).
- [llama.cpp grammars README](https://github.com/ggml-org/llama.cpp/blob/master/grammars/README.md) — `response_format` json_schema (the constraint the toolbench measures under `--protocol grammar`).
- [Gemma 3n model overview (Google)](https://ai.google.dev/gemma/docs/gemma-3n) — confirms "gemma-4-e4b" = Gemma 3n E4B, natively image/audio/video (drives the sprint-9 multimodal case; the toolbench's verdict thresholds should not assume multimodal).

## 4. Risks, Unknowns, Dependencies
- **Risk — cross-platform child-process management (Windows).** No SIGTERM; use `std::process::Command` + `Child::kill()` and store the PID in the runfile so `ferric server down` can stop a server started by a prior invocation. Mitigation: a small `ServerHandle` abstraction; on Windows, `Child::kill` maps to `TerminateProcess`. Validate the engine binary path (don't exec arbitrary input — ADR-005).
- **Risk — server readiness detection.** Poll `GET {base_url}/health` (llama-server) or `/v1/models` (both) until 200 or a timeout (e.g. 60 s); surface a clear error, never hang. Mitigation: bounded poll with backoff, like the loop's `complete_with_backoff`.
- **Risk — security (ADR-005).** The launcher MUST default to `--host 127.0.0.1` and never expose the server. The runfile lives under the workspace `.ferric/` (already a write-denied trace dir — keep server.json adjacent, not LLM-writable). The LLM never influences the server command.
- **Unknown — engine API divergence (llama-server vs Ollama).** Health endpoint and launch flags differ. Mitigation: an `Engine` trait with two impls (`LlamaServer`, `Ollama`); the OpenAI backend itself is unchanged (both speak `/v1/chat/completions`).
- **Unknown — failure taxonomy precision.** Distinguishing "wrong tool" from "malformed args" requires validating parsed args against the tool's `input_schema`. Mitigation: a lightweight required-keys/type check (not full JSON-Schema validation) is enough to classify; note the limit in the report.
- **Dependency — the mistral.rs viability run and any real-model toolbench report need a model/server** = the human heartbeat (the user provides it; `ferric server up` makes it one command). Model-free unit tests (MockProvider) cover the taxonomy + report logic.

## 5. Recommended Approach
**Primary — ship the diagnostic toolbench first (it's the priority and is independently valuable), then the launcher that makes it one command.**

1. **Diagnostic toolbench.** Replace `extract_action -> Option<ToolCall>` with `classify(protocol, completion, target, schema) -> Outcome`, where `Outcome ∈ { Success, WrongTool(name), MalformedArgs, NoAction, ParseError, ProviderError }`. Accumulate per-tool counts + a failure histogram. Emit (i) a live stdout view (the current `.`/`F`), (ii) a Markdown report mirroring `s6/toolbench-results.md` with the taxonomy, and (iii) a `toolbench-<model>.jsonl` machine-readable row per tool. Add an **overall verdict** band (e.g. ≥90 % solid / 70–90 % marginal / <70 % unreliable) so "dial the model down" has a readout. `--iterations`, `--protocol`, `--report <path>` flags.
2. **`ferric server` launcher.** New subcommand with an `Engine` trait (`LlamaServer` default, `Ollama`). `up` spawns the child (`llama-server -m … --mmproj … -c … --host 127.0.0.1 --port …`), polls `/health`, writes `.ferric/server.json` (`engine,pid,port,base_url`). `down` reads it and kills the PID. `status` health-checks. `doctor` validates binary+model presence and runs the constrained capability probe (E2E-1). `query`/`toolbench` default `--api-base` from the runfile when present.
3. **mistral.rs viability.** A build/test task that runs `grammar_probe` (trivial + unified schema) against 0.8.15 as a bounded subprocess; record the result in the test report and, per ADR-023, keep or deprioritize mistral.rs accordingly. (Real-model run = heartbeat.)

**Alternative considered — fold multimodal in now.** Rejected: multimodal is a cross-cutting `Message` content-parts + backend-mapping + capability-gating change (sprint 9, ADR-023). Bundling it would bloat this sprint and delay the diagnostic tool the user prioritized.

**Alternative considered — make the launcher a shell script, not a subcommand.** Rejected: the user explicitly wants it "a part of animus_ferric … a command," and a Rust subcommand gives cross-platform process management, the runfile contract, and `doctor` integration that a PS1/bash pair can't do portably.

**Rationale.** The diagnostic toolbench is the artifact that makes Ferric's whole "scale to the model" thesis *observable* — it turns "small models get small steps" from a claim into a per-model readout. The launcher removes the one manual step between a user and that readout. Both are pure additive Rust on the CLI surface; neither touches the loop/guard/trace core.

## Artifacts
- (referenced) `sprints/s6/sprint-tests/toolbench-results.md` — the prior-art report shape to reproduce + extend.
- (referenced) `decisions.md` ADR-023 — the launcher + testbench design this sprint implements.

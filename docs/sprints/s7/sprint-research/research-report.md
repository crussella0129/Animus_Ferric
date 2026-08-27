# Sprint 7 Research Report

> Scope note: Sprint 7 was initialized with the narrow objective "Cure
> Toolbench — fix three backend failures." This report **reframes** that
> objective. The three toolbench failures are *symptoms*; the disease is that
> the codebase has silently abandoned its own founding thesis (harness-owned
> constrained decoding) and grown a non-Rust crutch (the PyO3/PyTorch backend)
> in its place. Per the sprint directive — "make the project workable and
> aligned with original intent, even if that means a total refactor… plan good
> architecture rather than patching problems" — the recommended approach is a
> targeted re-alignment, not another patch. The existing
> `root-cause-analysis.md` (symptom-level) is retained as a supporting artifact.

## Decisions Reviewed

- **2026-06-10 ADR-001 — dual-backend, harness-owns-decoding** (sprint 0) — relevance: **the anchor.** Declares two backends (mistral.rs in-process + OpenAI-compatible HTTP escape valve) and that the harness owns decoding. The HTTP valve was created *precisely* as the escape hatch for when an in-process path fails. The project drifted off this; the cure is to lean on it. **Reaffirmed, not revised.**
- **2026-06-10 ADR-003 — Provider trait is constraint-carrying** (sprint 0) — relevance: **silently violated.** `CompletionRequest` no longer has a `Constraint` field. The constraint-carrying contract was deleted from the type system. **This sprint proposes to reinstate it.**
- **2026-06-11 ADR-010 — constraint XOR native tools, enforced by `validate()`** (sprint 1) — relevance: **silently violated.** `CompletionRequest::validate()` is now `Ok(())` — a no-op. The invariant is unenforced because the field it guarded no longer exists. **Reinstate.**
- **2026-06-13 ADR-015 — UnifiedGrammar via ActionProtocol** (sprint 2) — relevance: **degraded to a misnomer.** Today `ActionProtocol::UnifiedGrammar` sends **no constraint to any backend**; the loop emits a `ConstraintApplied` trace event that is false, then scrapes tool calls out of raw text with a **regex** (`grammar.rs`). That is the "repair malformed output" approach the thesis explicitly rejects. **Propose to amend (below).**
- **2026-06-11 ADR-013 — no new non-Rust residue without a new ADR** (sprint 1) — relevance: **violated by process.** The PyO3/PyTorch `PythonProvider` (s6) imported an entire embedded Python/C++ runtime into the agent process with **no governing ADR**. It also crashes (`STATUS_HEAP_CORRUPTION`). **Propose to remove it.**
- **2026-06-10 ADR-001 (llama.cpp FFI rejected)** — relevance: the same rationale that rejects llama.cpp FFI ("imports C++ UB into the agent process") condemns the in-process PyO3/PyTorch backend even harder. The *sanctioned* way to reach those engines is the out-of-process HTTP valve.
- **2026-06-15 ADR-020 — grammar opt-in pending mistral.rs hang root-cause** (sprint 2) — relevance: **the trigger of the whole drift.** `mistralrs::send_chat_request` + `Constraint::JsonSchema` hangs on GGUF (even a trivial schema); the tokenizer.json hypothesis was **DISPROVEN** (git `815bfb6`). This sprint proposes to amend ADR-020: grammar is not dead, it simply must live on the backend that can actually enforce it (HTTP), while mistral.rs stays native-only pending an upstream fix.

- **2026-06-13 ADR-017 — HTTP escape-valve + `validated_complete` wrapper** (sprint 2) — relevance: **directly realized here.** ADR-017 deferred the OpenAI HTTP backend with the explicit intent that it land alongside "a shared `validated_complete` wrapper that makes ADR-010 backend-boundary enforcement model-free testable." This sprint *is* that realization: the HTTP backend becomes the constraint-honoring path and `validate()` is re-enforced. **Fulfilled, not revised.**
- **2026-06-10 ADR-006 — scale function is pure, deterministic, config-fed** (sprint 0) — relevance: **touched but invariant-preserving.** T-004 edits `crates/ferric-core/src/scale.rs` *only* to rename the `ActionProtocol` enum variants; `policy_for`, `tier_row`, and the snapshot-pinned table are untouched. ADR-006's purity/determinism invariant is unaffected.

No prior decision is being *discarded*; the proposal **restores** ADR-001/003/010 to the code, **fulfills** ADR-017, and **amends** ADR-015/020 to route constrained decoding through the backend that works.

## 1. Sprint Goal

Make Animus Ferric do the one thing it was founded to do — **own decoding so a small local model cannot emit a malformed tool call** — on a path that demonstrably works, and purge the code that contradicts that thesis. Concretely: (a) reinstate the `Constraint` on `CompletionRequest` and re-enforce ADR-010 in `validate()`; (b) make the OpenAI-compatible HTTP backend carry a harness-authored JSON-Schema constraint via `response_format`, so the server (llama.cpp/Ollama) enforces it and small-model fire rate approaches 100% by construction; (c) **delete the PyO3/PyTorch Python backend and its `inference.py`** (ADR-013 violation + crash) — the HTTP backend already subsumes everything it tried to do, correctly and in pure Rust; (d) make every backend's `capabilities()` tell the truth about the code path that actually runs; (e) rebuild the toolbench to measure *constrained* fire rate vs *unconstrained native* fire rate, turning it into the evidence that the thesis holds. The mistral.rs in-process path is kept as the native-tools (unconstrained) flagship with its 300 s kill-switch, honestly labeled, pending an upstream llguidance fix.

## 2. Existing Code Survey

| File | Relevance | Notes |
|------|-----------|-------|
| crates/ferric-provider/src/types.rs | high | `CompletionRequest{messages,sampling,tools}` — **`Constraint` field deleted**; `validate()` is a no-op `Ok(())`. The thesis is gone from the type system. |
| crates/ferric-provider/src/traits.rs | high | `Provider::complete` is the one chokepoint; clean async/dyn surface — the right place to thread a constraint back through. |
| crates/ferric-provider/src/mistralrs.rs | high | `complete()` passes **neither tools nor grammar** ("s3 pivot"), yet `capabilities()` reports `supports_native_tool_calls: true`. The capability is a lie → toolbench Bug 1 (0.0%). Has a 300 s timeout (good). |
| crates/ferric-provider/src/openai.rs | high | Sends `tools`+`tool_choice:auto` but **no `response_format`/grammar**. The escape valve exists but does *not* yet carry a harness constraint — the missing keystone. Pure-Rust reqwest. |
| crates/ferric-provider/src/python.rs | high | PyO3 embeds CPython+PyTorch in-process; `tokio::spawn_blocking`+`Python::with_gil` cycles corrupt the Windows heap. Hardcodes `tool_calls:[]`. **Delete.** |
| crates/ferric-provider/python/inference.py | high | Non-Rust residue, no ADR. **Delete with python.rs.** |
| crates/ferric-provider/src/lib.rs | medium | Module doc claims "constraint plumbing"; exports **no `Constraint`**. Feature wiring for `backend-python` to be removed. |
| crates/ferric-provider/tests/grammar_probe.rs | medium | References `ferric_provider::Constraint` and `request.constraint` — types that no longer exist. **Won't compile under `backend-mistralrs`**; proof the constraint was ripped out, not refactored. |
| crates/ferric-provider/Cargo.toml | medium | `pyo3 0.23` under `backend-python`; `reqwest` under `backend-openai`. Drop pyo3; keep reqwest. |
| crates/ferric-loop/src/run.rs | high | Loop builds request per `ActionProtocol`; in `UnifiedGrammar` it emits `Event::ConstraintApplied` while sending **zero** constraint, then parses text via regex. Honesty + real-constraint wiring needed here. |
| crates/ferric-loop/src/grammar.rs | high | Regex XML scraper `<tool_call><name>..</name><args>..</args>`. This *is* the "repairable malformed output" the thesis rejects. Becomes a **fallback-only** path for backends with no constraint support, honestly traced. |
| crates/ferric-loop/src/protocol.rs | high | `select_protocol` hardwired to `NativeTools`, `_caps` ignored. Re-enable: pick `UnifiedGrammar` when the backend's `capabilities()` reports real constraint support (HTTP), else `NativeTools`. |
| crates/ferric-core/src/scale.rs | medium | `Protocol::ConstrainedJson` is the policy default but unused downstream; `ActionProtocol{NativeTools,UnifiedGrammar}`. Scale function itself is sound and well-tested — keep. |
| crates/ferric-cli/src/backend.rs | medium | `BackendArg{Mistral,Openai,Python}` factory. Remove `Python`; thread constraint config into OpenAI path. |
| crates/ferric-cli/src/toolbench_cmd.rs | high | Success = exactly one native `tool_calls[0].name == tool`. Tests only the broken native path. Rebuild to measure constrained vs native fire rate, dual-path parse. |
| crates/ferric-cli/src/query.rs | medium | One-shot surface; backend selection + `HF_HUB_OFFLINE` + kill-switch wiring live here. |
| crates/ferric-cli/tests/l0_smoke.rs | medium | The ADR-009 real-GGUF gate (native + grammar variants). The E2E acceptance anchor for the rebuilt path. |

## 3. External Sources

- [llama.cpp grammars README (ggml-org/llama.cpp)](https://github.com/ggml-org/llama.cpp/blob/master/grammars/README.md) — confirms server-side GBNF grammar + JSON-Schema→GBNF conversion; the `/v1/chat/completions` endpoint accepts `response_format: {type:"json_schema", json_schema:{schema:…}}`. **Load-bearing**: the harness can author the constraint and the server enforces it, in a separate process.
- [Grammar and Structured Output — DeepWiki (ggml-org/llama.cpp)](https://deepwiki.com/ggml-org/llama.cpp/8.1-grammar-and-structured-output) — details the supported JSON-Schema Draft-7 subset (types, `minLength/maxLength/pattern`, `minItems/maxItems`, `properties/required/additionalProperties`) and notes `additionalProperties:false` is the default for speed/anti-hallucination — matching the s2 action-schema shape.
- [llama.cpp issue #11847 — `response_format` on `/v1/chat/completions`](https://github.com/ggml-org/llama.cpp/issues/11847) — corroborates the OpenAI-compatible structured-output field and its quirks (schema constrains output but is **not** injected into the prompt → the harness must still describe tools in the system prompt; informs the prompt-composition task).

## 4. Risks, Unknowns, Dependencies

- **Risk — re-introducing the constraint must not regress mistral.rs into the hang.** Mitigation: `Constraint` is *carried* by the request but a backend only acts on it if its `capabilities()` advertises enforcement. mistral.rs advertises `false` and ignores the constraint (or is rejected by `validate()` if it claims native tools + constraint together). The hang path is never re-entered by default. The 300 s kill-switch stays.
- **Risk — the schema-not-in-prompt behavior.** Per source #3, llama.cpp constrains output to the schema but does not show the model the schema. Mitigation: keep the harness-authored system prompt describing each tool (ferric-prompt), and put the *action* JSON-Schema in `response_format`. The two are complementary, not redundant.
- **Risk — deleting the Python backend orphans Gemma-4-e4b coverage.** Mitigation: none needed — Gemma-4-e4b runs behind Ollama/llama-server and is reached through the OpenAI backend (`--backend openai --model gemma…`). This is strictly better than the crashing embedded path and is the ADR-001-sanctioned route.
- **Unknown — does the OpenAI backend's target server actually honor `response_format` at the user's chosen endpoint (LM Studio :1234 vs Ollama :11434 vs llama-server)?** Mitigation: the build plan's first task is a **capability probe** (one constrained request, assert the response validates against the schema) so a server that silently ignores `response_format` is detected, not trusted. This is the ADR-009 real-model gate applied to the HTTP path.
- **Unknown — exact `response_format` wire shape across servers** (`json_schema` vs `json_object`+`schema`). Resolve empirically in the probe; default to the documented `{type:"json_schema", json_schema:{name, schema, strict}}`.
- **Dependency — a running OpenAI-compatible server** for the E2E acceptance gate (human-launchable: `ollama serve` or `llama-server`). This is the visual-heartbeat checkpoint.
- **Dependency — toolchain.** `rust-toolchain.toml` now pins channel **1.96** (an earlier E2E log's `rustc 1.93.1 not supported` error is stale). Default workspace build stays mistralrs/pyo3/tokio-free; the aarch64 gate is unaffected by any change here (HTTP backend is pure Rust + reqwest, already allowlisted).

## 5. Recommended Approach

**Primary — Re-align to the thesis on the backend that can carry it; delete the backend that contradicts it.**

1. **Reinstate `Constraint` on `CompletionRequest`** (`enum Constraint { JsonSchema(Value), Regex(String), Lark(String) }`) and restore `validate()` to enforce ADR-010 (reject constraint+native-tools in the same request). Re-export `Constraint`; fix `grammar_probe.rs` to compile.
2. **Make the OpenAI HTTP backend constraint-honoring**: when `request.constraint` is `JsonSchema`, emit `response_format:{type:"json_schema", json_schema:{…, strict:true}}` and send tools empty; advertise `capabilities().supports_constraint = true`. This is the path where "harness owns decoding" is *true*, works today for 1B–14B GGUF, runs out-of-process (no C++ UB in the agent), and is 100% safe Rust on Ferric's side.
3. **Tell the truth in `capabilities()`**: mistral.rs reports `supports_native_tool_calls` honestly for the path it runs and `supports_constraint:false` (it hangs). No backend advertises a capability its `complete()` does not exercise. Capability is a structural contract, not aspiration.
4. **Re-enable `select_protocol` to read `capabilities()`**: prefer `UnifiedGrammar` (real constraint) when the backend enforces it; fall back to `NativeTools` otherwise. The regex `grammar.rs` becomes the *honest* last-resort parser for native/unconstrained text only, and the loop stops emitting a false `ConstraintApplied`.
5. **Delete `python.rs`, `inference.py`, the `backend-python` feature, `BackendArg::Python`, and the `pyo3` dependency.** Record the removal as an ADR (closing the ADR-013 process gap).
6. **Rebuild the toolbench** to report, per tool, *constrained fire rate* (HTTP+schema, expected ≈100%) and *native fire rate* (unconstrained, the real model-capability signal), with dual-path parsing (native `tool_calls`, else regex on text). This makes the benchmark the proof the thesis works rather than a test of the broken path.

**Alternative considered — Root-cause and fix the mistral.rs/llguidance hang so the in-process path carries the grammar (the "pure pure-Rust" ideal).** Rejected for *this* sprint: the hang is downstream in mistral.rs 0.8.1 / llguidance toktrie construction over a GGUF-synthesized tokenizer; the tokenizer.json hypothesis is already DISPROVEN (git `815bfb6`), and five sprints of evidence say it is an upstream fix, not a Ferric fix. It stays a tracked backlog item (minimal upstream repro, dep bump watch), not a blocker. Shipping the thesis through the HTTP valve *now* is exactly what ADR-001 designed the valve for.

**Alternative considered — the in-flight s7 plan (Python→subprocess HTTP server).** Rejected: it re-implements the OpenAI HTTP backend with a worse, hand-rolled, stdlib-`http.server` protocol and *keeps Python in the tree*. It is patching the crash instead of removing its cause. The OpenAI backend already is that subprocess-HTTP design, done correctly in Rust.

**Rationale.** Bad code here is not neutral: the false `capabilities()` and false `ConstraintApplied` actively mislead every future debugging session, the regex scraper silently lowers the ceiling of what small models can do (the very models Ferric exists to serve), and the PyO3 backend both crashes and breaks the auditable-ownership property that is the project's reason to be in Rust at all. Restoring the constraint to the one backend that enforces it deletes more code than it adds, makes the invalid states unrepresentable again, and turns the benchmark green for the right reason.

## Artifacts

- `root-cause-analysis.md` — symptom-level analysis of the three toolbench failures (Bug 1 native-tools stripped, Bug 2 heap corruption, Bug 3 toolbench has no text parser). Supporting evidence; this report supersedes its *recommendations* (which kept Python via subprocess).
- (referenced) `sprints/s6/sprint-tests/toolbench-results.md` — the 0.0% / heap-corruption run that triggered this sprint.
- (referenced) `decisions.md` ADR-020 — the mistral.rs grammar-hang finding that started the drift.

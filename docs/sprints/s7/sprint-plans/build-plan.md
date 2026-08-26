Finalized - DO NOT EDIT

# Sprint 7 Build Plan — Re-align Ferric to the Constrained-Decoding Thesis

> Supersedes the earlier "cure toolbench" draft. Direction confirmed with the
> user this session: restore harness-owned constrained decoding on the HTTP
> escape valve (ADR-001); delete the PyO3 backend (ADR-013 realignment).
> Rationale + evidence: `sprints/s7/sprint-research/research-report.md`.

## Schema Tree
- **Sprint Goal:** Restore harness-owned constrained decoding on the HTTP valve; purge the thesis-violating PyO3 backend; make `capabilities()` honest.
  - **Component A — Constraint contract** (ferric-provider, default graph)
    - T-001: Reinstate `Constraint` on `CompletionRequest`; re-enforce ADR-010 in `validate()`
  - **Component B — HTTP backend enforces the constraint** (backend-openai)
    - T-002: `OpenAiProvider` emits `response_format` for a JSON-Schema constraint
  - **Component C — Honest action protocols** (ferric-loop + ferric-core)
    - T-003: Unified action schema + JSON action parser
    - T-004: Protocol trichotomy (`NativeTools | ConstrainedJson | TextXml`) wired through the loop
  - **Component D — Remove the PyO3 backend** (ADR-013 realignment)
    - T-005: Delete the backend from `ferric-provider`
    - T-006: Remove it from the CLI + PS1 drivers
  - **Component E — Toolbench measures the real path**
    - T-007: Rebuild the toolbench around the active protocol's parser
  - **Component F — Honesty in docs + decisions**
    - T-008: Record ADR-021 + ADR-022; correct the lying docs

## Execution Sequence

### T-001: Reinstate `Constraint` on `CompletionRequest` and re-enforce ADR-010 in `validate()`
- **Touches:** `crates/ferric-provider/src/types.rs`, `crates/ferric-provider/src/lib.rs` (re-export); every `CompletionRequest { .. }` constructor gains `constraint: None` — `crates/ferric-provider/src/mock.rs`, `crates/ferric-loop/src/run.rs`, `crates/ferric-cli/src/{query.rs,toolbench_cmd.rs}`, `crates/ferric-bench/src/runner.rs` (if it constructs one), and affected `tests/*` + `tests/grammar_probe.rs`.
- **Depends on:** (none)
- **Success criterion (EARS):**
  - **WHEN** a request has `constraint = Some(_)` AND `!tools.is_empty()`, **THEN** `validate()` **SHALL** return `ProviderError::InvalidRequest`.
  - **WHEN** a request has `constraint = Some(_)` AND `tools.is_empty()`, **THEN** `validate()` **SHALL** return `Ok(())`.
  - **WHEN** a request has `!tools.is_empty()` AND `constraint = None`, **THEN** `validate()` **SHALL** return `Ok(())`.
  - **WHEN** a `Constraint::JsonSchema(v)` is serialized and deserialized, **THEN** the result **SHALL** equal the original.
- **Notes:** `enum Constraint { JsonSchema(serde_json::Value), Regex(String), Lark(String) }` (llguidance-shaped, ADR-003). Add `supports_constraint: bool` to `Capabilities`. Keystone task — everything else depends on the field existing. Reinstating the type also makes the stale `grammar_probe.rs` compile again. **Literal fan-out (C-005):** adding these fields forces every `Capabilities { .. }` literal (`mistralrs.rs`, `openai.rs`, `mock.rs`, test helpers incl. `ferric-loop/tests/common/mod.rs`) and every `CompletionRequest { .. }` literal to update in the *same* diff — the compiler enumerates them; lean on `cargo build` to find them all.

### T-002: Make `OpenAiProvider` emit `response_format` when a JSON-Schema constraint is present
- **Touches:** `crates/ferric-provider/src/openai.rs`
- **Depends on:** T-001
- **Success criterion (EARS):**
  - **WHEN** `complete()` is called with `Constraint::JsonSchema(s)`, **THEN** the request body **SHALL** contain `response_format.json_schema.schema == s` with `strict: true` and **SHALL NOT** contain `tools`.
  - **WHEN** called with tools and no constraint, **THEN** the body **SHALL** contain `tools`/`tool_choice` and **SHALL NOT** contain `response_format`.
  - **WHEN** `capabilities()` is read, **THEN** it **SHALL** report `supports_native_tool_calls: true` AND `supports_constraint: true`.
- **Notes:** Extract a pure `fn build_body(&self, &CompletionRequest) -> serde_json::Value` so the JSON shape is unit-testable with no network. Wire shape (llama.cpp / OpenAI structured outputs): `{"type":"json_schema","json_schema":{"name":"ferric_action","schema":<s>,"strict":true}}`.

### T-003: Add the unified action schema + JSON action parser to `ferric-loop`
- **Touches:** `crates/ferric-loop/src/grammar.rs` (or a new `action.rs`), `crates/ferric-loop/src/lib.rs` (export)
- **Depends on:** T-001
- **Success criterion (EARS):**
  - **WHEN** `action_schema(tools)` is built from N tool descriptors, **THEN** it **SHALL** produce an `anyOf` of N+1 const-discriminated `{tool, args}` branches (each tool + `task_complete`), each with `additionalProperties: false`.
  - **WHEN** `parse_json_action(turn, r#"{"tool":"read_file","args":{"path":"x"}}"#)` is called, **THEN** it **SHALL** return a `ToolCall` named `read_file` with id `g-<turn>-0`.
  - **WHEN** `parse_json_action` receives non-object JSON, or JSON missing `tool` or `args`, **THEN** it **SHALL** return a typed `ActionParseError`.
- **Notes:** Branch shape is already prototyped in `tests/grammar_probe.rs::branch()`. Leave the existing XML `parse_action` untouched — it backs the TextXml fallback in T-004.

### T-004: Replace the protocol dichotomy with an honest trichotomy and wire it through the loop
- **Touches:** `crates/ferric-core/src/scale.rs` (the `ActionProtocol` enum), `crates/ferric-loop/src/run.rs`, `crates/ferric-loop/src/protocol.rs`, `crates/ferric-cli/src/query.rs` (`--protocol` mapping), `crates/ferric-loop/tests/*` (grammar_loop/loop_core variants).
- **Depends on:** T-001, T-003
- **Success criterion (EARS):**
  - **WHEN** `select_protocol` sees `caps.supports_constraint`, **THEN** it **SHALL** return `ConstrainedJson`; **WHEN** only `caps.supports_native_tool_calls`, **THEN** it **SHALL** return `NativeTools`; **WHEN** neither, **THEN** it **SHALL** return `TextXml`; an explicit override **SHALL** always win.
  - **WHEN** protocol is `ConstrainedJson`, **THEN** the loop **SHALL** build a request carrying `Constraint::JsonSchema(action_schema)` with empty tools, **SHALL** emit `Event::ConstraintApplied`, and **SHALL** parse the completion via `parse_json_action`.
  - **WHEN** protocol is `TextXml`, **THEN** the request **SHALL** carry no constraint and no tools, the loop **SHALL NOT** emit `ConstraintApplied`, and it **SHALL** parse via the XML `parse_action`.
  - **WHEN** protocol is `NativeTools`, **THEN** the request **SHALL** carry tools and no constraint, and the loop **SHALL** read `completion.message.tool_calls`.
- **Notes:** `ActionProtocol { NativeTools, ConstrainedJson, TextXml }`. CLI `--protocol {native,grammar,xml}` → the three variants. Removes the false `ConstraintApplied` and the `UnifiedGrammar` misnomer. The rename forces all touched files into one coherent, compiling diff. **Prompt-enumerates-tools (C-002):** because the JSON-Schema is enforced server-side but **not** injected into the prompt (research §4), the `ConstrainedJson` path MUST use the composed `ferric-prompt` system prompt that enumerates the available tools and their input schemas — not the bare `DEFAULT_SYSTEM_PROMPT`. The constraint guarantees structure, not tool/arg *choice*; verify `ferric-prompt` already emits tool descriptions during build, and have `query.rs` pass the composed prompt through for all three protocols.

### T-005: Delete the PyO3 backend from `ferric-provider`
- **Touches:** delete `crates/ferric-provider/src/python.rs` and `crates/ferric-provider/python/inference.py`; edit `crates/ferric-provider/Cargo.toml` (drop the `backend-python` feature + `pyo3` dep) and `crates/ferric-provider/src/lib.rs` (drop the `python` module + its exports).
- **Depends on:** (none) — structurally independent; sequenced here to land before the CLI cleanup.
- **Success criterion (EARS):**
  - **WHEN** the workspace is built with `--features backend-openai,backend-mistralrs`, **THEN** it **SHALL** compile with no reference to `pyo3` or the `python` module.
  - **WHEN** `cargo tree -p ferric-provider --all-features` is inspected, **THEN** `pyo3` **SHALL NOT** appear.
- **Notes:** Git history retains the deleted code (reversible). The `lib.rs` module-doc lie ("constraint plumbing") is corrected in T-008.

### T-006: Remove the Python backend from the CLI and the PS1 drivers
- **Touches:** `crates/ferric-cli/src/backend.rs` (drop `BackendArg::Python` + its match arm), `crates/ferric-cli/src/{query.rs,toolbench_cmd.rs}` (drop `backend-python` cfg gates), root `test_both_models.ps1` and `run_benchmarks.ps1` (drop python invocations).
- **Depends on:** T-005
- **Success criterion (EARS):**
  - **WHEN** `ferric query --help` lists the `--backend` values, **THEN** `python` **SHALL NOT** appear.
  - **WHEN** `backend.rs` is compiled, **THEN** `BackendArg` **SHALL** contain only `Mistral` and `Openai`.
- **Notes:** Keep the `#[cfg(not(any(...)))]` fallback arms consistent with the reduced feature set.

### T-007: Rebuild the toolbench to measure the active protocol's real fire rate
- **Touches:** `crates/ferric-cli/src/toolbench_cmd.rs`
- **Depends on:** T-004, T-006
- **Success criterion (EARS):**
  - **WHEN** a completion yields a tool call via the active protocol's parser (`tool_calls` for `NativeTools`, `parse_json_action` for `ConstrainedJson`, `parse_action` for `TextXml`) whose name matches the target tool, **THEN** the iteration **SHALL** count as a pass.
  - **WHEN** no parser extracts a matching call, **THEN** the iteration **SHALL** count as a fail.
  - **WHEN** run with `--protocol grammar` against a constraint-honoring server, **THEN** the reported fire rate **SHALL** be derived from JSON `{tool,args}` parsing, not native `tool_calls`.
- **Notes:** Reuse the loop's protocol-selection + parse functions (public after T-003/T-004) so the bench measures exactly what `ferric query` does. Add `--protocol`.

### T-008: Record ADR-021 + ADR-022 and correct the lying docs
- **Touches:** `decisions.md`, `crates/ferric-provider/src/lib.rs` (module doc), `README.md` (Status section).
- **Depends on:** T-001, T-002, T-003, T-004, T-005, T-006, T-007
- **Success criterion (EARS):**
  - **WHEN** `decisions.md` is read, **THEN** it **SHALL** contain **ADR-021** (remove the PyO3 backend; external engines reached only via the out-of-process HTTP valve; closes the ADR-013 process gap) and **ADR-022** (constraint reinstated; ADR-010 re-enforced; honest `capabilities()`; protocol trichotomy; amends ADR-015/ADR-020).
  - **WHEN** `README.md` Status is read, **THEN** it **SHALL** describe the dual-backend constrained-decoding state, not "Sprint 0 — no inference backend yet."
- **Notes:** Per-task commits record the work; this task makes the architecture record match the code.

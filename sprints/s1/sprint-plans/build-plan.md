Finalized - DO NOT EDIT

# Sprint 1 Build Plan — First Real Backend + Production Loop

## Schema Tree
- Sprint Goal: Real inference (mistral.rs CPU/GGUF), production agent loop with lineage fixes, `ferric query` surface, L0 smoke E2E, vision ADRs + roadmap
  - Trace & guard groundwork
    - T-101: Extend trace vocabulary (6 new events) + render arms
    - T-102: Registry surfaces CheckRecords; protect `.ferric`
    - T-103: Provider request validation (ADR-010) + retryability
  - Production loop (ferric-loop)
    - T-104: Core turn loop
    - T-105: task_complete structured terminator
    - T-106: Hash-ALL-calls repetition guard
    - T-107: Exponential backoff
  - Real backend
    - T-108: Workspace deps + backend-mistralrs feature + CI wiring
    - T-109: MistralRsProvider
  - CLI surface
    - T-110: CLI graduates to clap
    - T-111: ferric query subcommand
  - Validation & direction
    - T-112: L0 smoke E2E
    - T-113: ADR-010..014 + backlog roadmap

## Execution Sequence

### T-101: Add TurnStart, TurnEnd, PromptAssembled, ConstraintApplied, RepetitionGuard, PermissionCheck events to ferric-trace and render arms to trace cat.
- **Touches:** `crates/ferric-trace/src/event.rs`, `crates/ferric-cli/src/main.rs`
- **Depends on:** (none)
- **Success criterion (EARS):**
  - **WHEN** a trace containing the six new event types is read by the reader, **THEN** it **SHALL** yield them as `Known` events with all fields round-tripping through serde.
  - **WHEN** `ferric trace cat` renders a file containing the new events, **THEN** it **SHALL** produce one human line per event with no `[unknown event]` fallbacks.
  - **WHEN** the schema version is inspected, **THEN** `TRACE_SCHEMA_VERSION` **SHALL** remain 1 (additive change per ADR-002).
- **Notes:** Field schemas (locked here per critique C-003): `TurnStart { turn: u32 }`; `TurnEnd { turn: u32, text: Option<String>, tool_call_count: u32, input_tokens: Option<u32>, output_tokens: Option<u32> }` (closes the s0 gap where assistant text was never traced); `PromptAssembled { turn: u32, message_count: u32, chars: u64, offered_tools: Vec<String> }`; `ConstraintApplied { kind: String }`; `RepetitionGuard { action: String }` (action ∈ warned|stopped); `PermissionCheck { path: String, decision: String, rule: Option<String>, matched: Option<String> }`.

### T-102: Surface permission checks from the registry chokepoint and deny writes under `.ferric`.
- **Touches:** `crates/ferric-tools/src/registry.rs`, `crates/ferric-guard/src/denylist.rs`, touched tests in `crates/ferric-tools/tests/`
- **Depends on:** (none)
- **Success criterion (EARS):**
  - **WHEN** `execute` completes or denies, **THEN** the outcome **SHALL** include one `CheckRecord { path, decision, rule, matched }` per declared target path.
  - **WHEN** a tool attempts a write under any `.ferric` segment, **THEN** the guard **SHALL** deny with rule `denied_write_segment`.
  - **WHEN** a denial occurs, **THEN** the tool handler **SHALL NOT** run (existing invariant preserved).
- **Notes:** Checker logic unchanged; the loop traces what already happened. Trace self-protection per ADR-005 pattern. `CheckRecord { path: PathBuf, decision: Decision-shaped string, rule: Option<&'static str>, matched: Option<String> }` is DEFINED in registry.rs (critique C-002); `ExecuteOutcome::Completed` and `::Denied` gain a `checks: Vec<CheckRecord>` field.

### T-103: Add CompletionRequest::validate() (constraint×tools exclusivity), the new ProviderError::RetryableBackend variant, and is_retryable().
- **Touches:** `crates/ferric-provider/src/types.rs`
- **Depends on:** (none)
- **Success criterion (EARS):**
  - **WHEN** a `CompletionRequest` carries both a constraint and a non-empty tool list, **THEN** `validate()` **SHALL** return `InvalidRequest` naming the conflict.
  - **WHEN** `is_retryable()` is asked about each error variant, **THEN** it **SHALL** return true for `RetryableBackend` and false for `ScriptExhausted`, `Backend`, and `InvalidRequest`.
- **Notes:** ADR-010 enforcement point. `RetryableBackend(String)` is a NEW variant added here (critique C-001); semantics: `Backend` = permanent (load/parse/config errors), `RetryableBackend` = transient (timeouts, channel disconnects). The loop calls `request.validate()` before every `provider.complete()` (primary enforcement); backends validate again at their boundary (defense in depth, critique C-006).

### T-104: Create the ferric-loop crate with the core turn loop productionizing mock_loop_skeleton.
- **Touches:** `Cargo.toml`, `crates/ferric-loop/Cargo.toml`, `crates/ferric-loop/src/{lib.rs,run.rs,outcome.rs}`
- **Depends on:** T-101, T-102
- **Success criterion (EARS):**
  - **WHEN** the scripted mock emits a tool call then a text completion, **THEN** the loop **SHALL** return that text with `StopReason::FinalText` and the trace **SHALL** contain, in seq order: session_start, turn_start, prompt_assembled, turn_end, tool_call, permission_check, tool_result, turn_start, prompt_assembled, turn_end, session_end.
  - **WHEN** the turn count reaches `policy.max_turns` without a terminator, **THEN** the loop **SHALL** stop with `StopReason::MaxTurns` and write `SessionEnd { reason: "max_turns" }`, still emitting the last assistant text as best-effort output.
  - **WHEN** a tool dispatch is denied or unknown, **THEN** the loop **SHALL** feed the failure back as an is_error tool-result message and continue.
- **Notes:** Executor-agnostic (no tokio); sees only `dyn Provider`; injectable `Sleeper` defined here for T-107. Edge cases pre-decided: empty completion → nudge once then stop ("empty_completion"); text+tools → execute and continue. T-104 is the core scaffold (lib.rs/run.rs/outcome.rs); T-105..T-107 ADD modules (terminator.rs/repetition.rs/backoff.rs) and extend run.rs to integrate them — their Depends-on lines express this (critique C-008). The loop calls `request.validate()` before each provider call (C-006).

### T-105: Implement the task_complete structured terminator.
- **Touches:** `crates/ferric-loop/src/{terminator.rs,run.rs}`
- **Depends on:** T-104
- **Success criterion (EARS):**
  - **WHEN** the model calls `task_complete`, **THEN** the loop **SHALL** stop with `StopReason::TaskComplete`, final text = the summary argument, without invoking `Registry::execute` for that call, writing `SessionEnd { reason: "task_complete" }`.
  - **WHEN** `task_complete` arrives alongside other tool calls in the same turn, **THEN** the loop **SHALL** execute the other calls first and then terminate.
  - **WHEN** tool-turn requests are assembled, **THEN** the `task_complete` descriptor **SHALL** be included even when `tools_for_policy` already fills `max_tools`.
- **Notes:** Never a registered tool — intercepted by name (lineage pattern). Malformed/missing summary → terminate with empty summary, not a dispatch-error loop.

### T-106: Implement the hash-ALL-calls repetition guard.
- **Touches:** `crates/ferric-loop/src/{repetition.rs,run.rs}`
- **Depends on:** T-104
- **Success criterion (EARS):**
  - **WHEN** two consecutive turns issue identical tool-call sets (names + canonical args, ids excluded), **THEN** the loop **SHALL** trace `RepetitionGuard { action: "warned" }` and inject a nudge message before the next request.
  - **WHEN** a third consecutive identical set arrives, **THEN** the loop **SHALL** stop with `SessionEnd { reason: "repetition_guard" }`.
  - **WHEN** call sets differ in any name, argument, or order, **THEN** the guard counter **SHALL** reset.
- **Notes:** Prion failure-mode #5 (hash ALL calls, not just first). Known calibration item: poll-style legitimate repeats; two-strike design is the mitigation.

### T-107: Implement exponential backoff on retryable provider errors.
- **Touches:** `crates/ferric-loop/src/{backoff.rs,run.rs}`
- **Depends on:** T-104
- **Success criterion (EARS):**
  - **WHEN** the provider fails with retryable errors N≤3 times then succeeds, **THEN** the loop **SHALL** complete normally and the recording sleeper **SHALL** have observed delays 250/500/1000 ms.
  - **WHEN** retries are exhausted, **THEN** the loop **SHALL** stop with `StopReason::ProviderError` and `SessionEnd { reason: "provider_error" }`.
  - **WHEN** the error is non-retryable, **THEN** the loop **SHALL NOT** sleep and **SHALL** abort immediately.
- **Notes:** Prion failure-mode #6. Injectable Sleeper (default std::thread::sleep — engine runs on its own OS thread).

### T-108: Wire workspace dependencies, the backend-mistralrs feature, and the CI backend-check job.
- **Touches:** `Cargo.toml`, `crates/ferric-provider/Cargo.toml`, `crates/ferric-cli/Cargo.toml`, `.github/workflows/ci.yml`
- **Depends on:** (none; blocks T-109/T-110)
- **Success criterion (EARS):**
  - **WHEN** `cargo check --workspace --target aarch64-unknown-linux-gnu` runs with default features, **THEN** the dependency graph **SHALL NOT** contain mistralrs or tokio.
  - **WHEN** `cargo clippy -p ferric-cli --features backend-mistralrs --all-targets -- -D warnings` runs, **THEN** it **SHALL** compile the gated code and pass.
  - **WHEN** the default `cargo test --workspace` runs, **THEN** behavior **SHALL** be unchanged (no new deps compiled).
- **Notes:** `mistralrs = "=0.8.1"`, tokio (cli, feature-gated), clap 4 derive (cli, unconditional), futures-executor promoted to ferric-cli regular dep. New linux-only CI job clippy-checks the feature (cold ~15–30 min, rust-cache warms to ~3–6).

### T-109: Implement MistralRsProvider behind the feature flag.
- **Touches:** `crates/ferric-provider/src/mistralrs.rs`, `crates/ferric-provider/src/lib.rs`
- **Depends on:** T-103, T-108
- **Success criterion (EARS):**
  - **WHEN** a request carries both constraint and tools, **THEN** `complete` **SHALL** return `InvalidRequest` without contacting the engine.
  - **WHEN** each `Constraint` variant is mapped, **THEN** the system **SHALL** produce the corresponding `mistralrs::Constraint::{JsonSchema, Regex, Lark}` 1:1.
  - **WHEN** a response contains usage / tool calls, **THEN** the returned `Completion` **SHALL** carry both token counts and round-tripped `ferric_core::ToolCall`s with JSON-parsed args.
- **Notes:** Per `mistralrs-integration-spec.md`: GgufModelBuilder(dir, files), TokenSource::None, HF_HUB_OFFLINE=1 (unsafe set_var in main before threads — edition 2024), with_force_cpu, with_max_num_seqs(2), strict native tools + ToolChoice::Auto, set_deterministic_sampler when temperature == 0. Mapping in free functions (model-free unit tests). Budget for minor 0.8.1-source API drift vs the spec. Error classification (critique C-007): transient engine errors (request-channel disconnects, timeouts) → `RetryableBackend`; model-load/GGUF/template errors → `Backend` (permanent).

### T-110: Graduate the CLI to clap (trace cat preserved byte-for-byte).
- **Touches:** `crates/ferric-cli/src/{main.rs,trace_cmd.rs}`, `crates/ferric-cli/Cargo.toml`, `crates/ferric-cli/tests/cli.rs`
- **Depends on:** T-101, T-108
- **Success criterion (EARS):**
  - **WHEN** `ferric trace cat <file>` runs on an s0-format trace, **THEN** output **SHALL** be unchanged from the s0 binary.
  - **WHEN** `ferric` runs with no or unknown args, **THEN** it **SHALL** print clap usage and exit non-zero.
- **Notes:** `query` flags defined here, handler stubbed to T-111.

### T-111: Implement the ferric query subcommand.
- **Touches:** `crates/ferric-cli/src/{main.rs,query.rs}`, `crates/ferric-cli/Cargo.toml`
- **Depends on:** T-104..T-107, T-109, T-110
- **Success criterion (EARS):**
  - **WHEN** `ferric query --mock "x"` runs in a temp dir, **THEN** it **SHALL** exit 0, print the mock's final text, and leave a parseable trace at `.ferric/trace/q-*.jsonl` spanning session_start..session_end.
  - **WHEN** `query` runs without `--mock` in a build lacking backend-mistralrs, **THEN** it **SHALL** exit non-zero naming the missing feature.
  - **WHEN** `--workspace` is omitted, **THEN** the current working directory **SHALL** be the containment boundary.
- **Notes:** Flags: prompt, --workspace, --model-dir, --model-file, --ctx, --params-b, --quant, --family, --chat-template, --mock. ModelProfile config-supplied (ADR-006). Executor boundary (critique C-009): `--mock` drives the loop via `futures_executor::block_on` (no tokio in the default build); the real path constructs a tokio multi-thread Runtime and `block_on`s the loop there (mistralrs client futures need ambient tokio). The L0 smoke avoids executor contention entirely by spawning the binary as a separate process. Session id `q-<unix_ms>`.

### T-112: Write the L0 smoke E2E test.
- **Touches:** `crates/ferric-cli/tests/l0_smoke.rs`
- **Depends on:** T-111
- **Success criterion (EARS):**
  - **WHEN** run with a valid Llama-3.2-1B GGUF (FERRIC_SMOKE_MODEL_DIR/FILE set, `--ignored`, feature on), **THEN** the test **SHALL** pass all eight assertions in the test plan §E2E in a single real-GGUF run (ADR-009 gate).
  - **WHEN** the env vars are absent, **THEN** the test **SHALL** fail fast with an instructive message.
- **Notes:** Spawns the binary via CARGO_BIN_EXE (avoids driving mistralrs futures on the wrong executor). Deterministic temperature. Records wall time + token counts for the test report.

### T-113: Record ADR-010..014 and rewrite the backlog roadmap.
- **Touches:** `decisions.md`, `agent-tasks/agent-tasks.md`, `agent-tasks/completed-tasks.md`
- **Depends on:** T-112
- **Success criterion (EARS):**
  - **WHEN** the sprint closes, **THEN** `decisions.md` **SHALL** contain ADR-010..014 dated and consecutively numbered, and every vision item from research §5.5 **SHALL** exist as a sprint-tagged backlog entry.
- **Notes:** ADR wording finalized in the plan-agent output (constraint×tools exclusivity; no chat catch-all; MCP-stdio-first; named ownership boundaries; pinned capability roadmap s2→s7). Also records the ADR-004 allowlist amendment (critique C-005): s1 adds mistralrs =0.8.1 (feature-gated, default off), tokio (cli, feature-gated), clap 4 (cli, unconditional — the CLI surface is now real), futures-executor promoted to a ferric-cli regular dep; ADR-004's aarch64 gate invariant unchanged.

## Lineage-Fix Ledger update (s1 dispositions)

| Deferred-from-s0 fix | s1 disposition |
|---|---|
| Hash-ALL-calls repetition guard | **T-106** ✓ |
| Structured terminator (task_complete) | **T-105** ✓ |
| Exponential backoff on retryable errors | **T-107** ✓ |
| First real backend + L0 smoke | **T-109/T-112** ✓ |
| Bounded HTTP reads | deferred → s2 (HTTP escape-valve backend) |
| Stale-config detection/migration | deferred → s2 (config crate arrives with oovra integration) |
| Circuit-breaker compaction | deferred → s2 (context manager) |

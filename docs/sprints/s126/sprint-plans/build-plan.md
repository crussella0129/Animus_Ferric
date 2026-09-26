# Sprint 126 Build Plan

## Intents
- [INT-0011](../../../intents/INT-0011-standalone-constrained-decoding-core.md) — state: planned; acceptance criteria covered: AC-1 (dependency boundary), AC-2 (consumers use the core, suite unchanged), AC-3 (OpenAI-tools adapter part only), AC-4 (no silently dropped constraint).
- [INT-0012](../../../intents/INT-0012-constrained-valve-at-hermes-boundary.md) — state: planned; acceptance criteria covered: AC-1 (contract and translation), AC-2 (streaming and liveness), AC-3 (cancellation), AC-4 (receipts), AC-5 (model-free conformance), AC-6 (startup enforcement probe), AC-7 (live readiness smoke), plus the record-only boundary.
- [INT-0013](../../../intents/INT-0013-constrained-decoding-on-midsize-quantized-target.md) — state: planned; acceptance criteria covered: AC-1 (manifest) and part of AC-3 (paired, counterbalanced, 3 repetitions), limited to a two-arm pilot (native versus F-thought).
- [INT-0010](../../../intents/INT-0010-ferric-is-the-iron-of-amalgam.md) — state: planned; acceptance criteria covered: AC-1 (docs retarget), AC-3 (ledger triage), AC-5 (project instructions). AC-6 (green `main`) follows from the carried-over fix `bf3c7d8` landing in this sprint's PR.

## Schema Tree
- Sprint Goal: real Hermes runs through Ferric's constrained valve, built on an extracted core, with the extraction and a native-vs-valve pilot measured
  - Stage A — Extraction (gate before Stage B)
    - T-12601: create `ferric-iron` and move the constrained protocol into it
    - T-12602: dependency-boundary test and the extraction measurement gate
    - T-12603: capability honesty — reject unenforceable `Regex` constraints
    - T-12604: OpenAI-tools adapter
  - Stage B — Valve
    - T-12605: pure request transform
    - T-12606: upstream streaming and response translation
    - T-12607: receipts, startup probe, record-only mode and CLI
  - Stage C — Real Hermes end to end
    - T-12608: source-defined E2E runner and Hermes driver; 7B bring-up and 27B readiness
    - T-12609: pilot corpus, 27B run and report
  - Stage D — Charter
    - T-12610: charter documentation, instructions and ledger triage

## Execution Sequence

### T-12601: Create `crates/ferric-iron` and move the constrained protocol into it without changing behavior
- **Intent:** [INT-0011](../../../intents/INT-0011-standalone-constrained-decoding-core.md)
- **Touches:**
  - new: `crates/ferric-iron/`
  - root: `Cargo.toml`, `Cargo.lock`
  - `crates/ferric-provider/src/{types.rs,stream_scan.rs,lib.rs}`
  - `crates/ferric-loop/src/{grammar.rs,terminator.rs,protocol.rs,run.rs,projector.rs,lib.rs}`
  - the Cargo manifests of `ferric-provider` and `ferric-loop`
- **Depends on:** (none)
- **Acceptance criterion:** INT-0011 AC-2. The core's consumers use it for schema authoring, parsing, protocol selection and control branches, the old in-crate implementations are removed, and the full workspace suite passes unchanged. `ferric-bench` uses none of the moved items (checked by grep), so the consumers this sprint are `ferric-loop`, `ferric-provider` and `ferric-cli` (`human.rs`, `query.rs`, `toolbench_cmd.rs`), all through re-exports.
- **Success criterion (EARS):**
  - **WHEN** the workspace builds after the move, **THEN** every item that `ferric-provider`'s `lib.rs` and `ferric-loop`'s `lib.rs` exported from the moved modules at `bf3c7d8` (types, `stream_scan`, `grammar` except `parse_action`, `terminator`, `protocol`) **SHALL** still resolve at its old path, through re-exports.
  - **WHEN** `cargo test --workspace --locked -- --test-threads=1` runs after the move, **THEN** its passed, failed and ignored totals **SHALL** equal the baseline recorded at `bf3c7d8` before the move, and no test **SHALL** be deleted.
  - **WHEN** `ferric-loop` renders the constrained system prompt or a constrained tool result, **THEN** it **SHALL** call `ferric_iron::render_tool_listing` and `ferric_iron::tool_result_text`, and the rendered bytes **SHALL** equal the pre-move bytes.
- **Notes:** Move code verbatim, tests included. Keep the XML `parse_action` (which needs regex) in `ferric-loop`. `ferric-iron` depends on `ferric-core` for `ActionProtocol`, `RunPolicy`, `ToolCall` and `UserInputRequest`. The scale function stays in `ferric-core` this sprint. Record the baseline suite totals before the first move commit. No fix rides along with this refactor.

### T-12602: Pin the core's dependency boundary and record the extraction measurements that gate Stage B
- **Intent:** [INT-0011](../../../intents/INT-0011-standalone-constrained-decoding-core.md)
- **Touches:** `crates/ferric-iron/tests/dependency_boundary.rs`, `docs/sprints/s126/sprint-tests/extraction-measurement.md`
- **Depends on:** T-12601
- **Acceptance criterion:** INT-0011 AC-1. The core builds with a dependency tree limited to serialization and error crates plus shared domain types, and a test pins that boundary.
- **Success criterion (EARS):**
  - **WHEN** `ferric-iron/Cargo.toml` `[dependencies]` contains a crate outside {ferric-core, serde, serde_json, thiserror}, or `ferric-core/Cargo.toml` `[dependencies]` contains a crate outside {serde, serde_json, thiserror}, **THEN** the boundary test **SHALL** fail and name the offending crate and manifest.
  - **WHEN** the extraction lands, **THEN** `extraction-measurement.md` **SHALL** record:
    - moved lines per source file;
    - the normal-edge crate counts from `cargo tree -p ferric-iron -e normal` and `cargo tree -p ferric-loop -e normal`;
    - clean build wall time of `cargo build -p ferric-iron` versus `cargo build -p ferric-loop` in a fresh target directory;
    - baseline versus post-move suite totals.
  - **WHEN** the boundary test fails or the post-move totals differ from the baseline, **THEN** no Stage B task **SHALL** start, and the blocker **SHALL** be reported to the owner before any alternative (copying code, Python middleware) is attempted.
- **Notes:** The test parses the TOML manifests with the workspace `toml` dependency as a dev-dependency. Dev-dependencies do not count against the boundary.

### T-12603: Reject a constraint the OpenAI-compatible backend cannot transmit instead of sending an unconstrained request
- **Intent:** [INT-0011](../../../intents/INT-0011-standalone-constrained-decoding-core.md)
- **Touches:** `crates/ferric-provider/src/openai.rs`
- **Depends on:** T-12601
- **Acceptance criterion:** INT-0011 AC-4. No code path drops a requested constraint and sends an unconstrained request.
- **Success criterion (EARS):**
  - **WHEN** `complete` or `complete_streaming` receives a request whose constraint is `Constraint::Regex`, **THEN** it **SHALL** return `ProviderError::InvalidRequest` naming the unsupported constraint, before any network I/O.
  - **WHEN** `build_body` receives a `JsonSchema`-constrained, a `Lark`-constrained, a tools-only or an unconstrained request, **THEN** the returned body **SHALL** be byte-identical to the pre-change body.
- **Notes:** A separate fix commit. `build_body` becomes fallible (`Result<Value, ProviderError>`).

### T-12604: Adapt an OpenAI-format `tools` array into ordered core descriptors with typed rejection
- **Intent:** [INT-0011](../../../intents/INT-0011-standalone-constrained-decoding-core.md)
- **Touches:** `crates/ferric-iron/src/openai_tools.rs`, `crates/ferric-iron/src/lib.rs`
- **Depends on:** T-12601
- **Acceptance criterion:** INT-0011 AC-3, adapter part. The core accepts an OpenAI-format `tools` array and produces an action schema from it.
- **Success criterion (EARS):**
  - **WHEN** `descriptors_from_openai_tools` receives `[{"type":"function","function":{"name","description","parameters"}}, …]`, **THEN** it **SHALL** return one `ToolDescriptor` per entry in input order. A missing `description` becomes the empty string, and a missing `parameters` becomes `{"type":"object","properties":{}}`.
  - **WHEN** an entry has a type other than `"function"`, a missing or empty name, a name duplicated within the array, or a name equal to a reserved control name (`task_complete`, `request_user_input`, `submit_plan`), **THEN** it **SHALL** return a typed error naming the entry index and name.
  - **WHEN** the returned descriptors plus the control descriptors are passed to `action_schema`, **THEN** the schema **SHALL** contain exactly one `anyOf` branch per descriptor, in order.
- **Notes:** Supported-construct fixtures (the rest of AC-3) are recorded from live E2E observation in T-12608 and T-12609, not guessed.

### T-12605: Transform a Hermes chat-completions request into a deterministic constrained upstream request, or classify it as pass-through
- **Intent:** [INT-0012](../../../intents/INT-0012-constrained-valve-at-hermes-boundary.md)
- **Touches:** new `crates/ferric-valve/` (`Cargo.toml`, `src/lib.rs`, `src/transform.rs`); root `Cargo.toml`, `Cargo.lock`
- **Depends on:** T-12602, T-12604
- **Acceptance criterion:** INT-0012 AC-1, the request half of the contract: the mapping from Hermes's request subset to the constrained upstream request, with unknown fields forwarded per a documented rule and deterministic prompt bytes.
- **Success criterion (EARS):**
  - **WHEN** a request has non-empty `tools`, a `tool_choice` other than `"none"` and no `response_format`, **THEN** `transform` **SHALL** return constrained mode with an upstream body that:
    - has `tools`, `tool_choice` and `parallel_tool_calls` removed;
    - has `response_format` set to `{"type":"json_schema","json_schema":{"name":"ferric_action","schema":<action_schema(tools + final-answer control)>,"strict":true}}`;
    - has the system message extended by the constrained-protocol teaching text plus `render_tool_listing`, or a system message inserted first when none exists;
    - carries every other top-level field unchanged, and forces `stream: true`.
  - **WHEN** a request has no or empty `tools`, `tool_choice: "none"`, or a `response_format`, **THEN** `transform` **SHALL** return pass-through mode with the body byte-identical to the input.
  - **WHEN** the history contains an assistant message with `tool_calls`, **THEN** it **SHALL** be projected to one assistant message whose content is the compact canonical action `{"thought":<reasoning_content or "">,"tool":<name>,"args":<parsed arguments>}`. A following `tool` message **SHALL** become a user message `tool_result_text(name, content)`, with the name resolved from `tool_call_id`.
  - **WHEN** the history contains an assistant message without `tool_calls`, **THEN** it **SHALL** be projected to the canonical final-answer action `{"thought":<reasoning_content or "">,"tool":"task_complete","args":{"summary":<content>}}`.
  - **WHEN** a `tool` message's `tool_call_id` matches no earlier assistant tool call, or an assistant tool call's `arguments` is not a JSON object, **THEN** `transform` **SHALL** return a typed error that the server maps to HTTP 400.
  - **WHEN** the same request is transformed twice, or a request is extended by appending turns, **THEN** the upstream bodies **SHALL** be byte-identical in the first case, and in the second the earlier request's list of per-message hashes over the rendered upstream messages (`message_hashes`) **SHALL** be a prefix of the later request's list.
- **Notes:**
  - The final-answer control reuses the name `task_complete` and the field `summary`, with its description changed to "your complete reply to the user", so the existing scanner streams it.
  - Canonical serialization is compact `serde_json` with `preserve_order`.
  - The transform is pure: no I/O and no clock.
  - A contract table in the crate's `README.md` documents forwarded, removed and rewritten fields.

### T-12606: Stream the constrained upstream response and translate it into OpenAI tool calls or content, honestly
- **Intent:** [INT-0012](../../../intents/INT-0012-constrained-valve-at-hermes-boundary.md)
- **Touches:** `crates/ferric-valve/src/{upstream.rs,translate.rs,sse.rs}`
- **Depends on:** T-12605
- **Acceptance criterion:** INT-0012 AC-1 (the response half), AC-2 (SSE streaming with visible liveness, and equivalent non-streaming content) and AC-3 (cancellation releases the upstream).
- **Success criterion (EARS):**
  - **WHEN** the upstream action parses to an offered tool, **THEN** the final response **SHALL** carry exactly one `tool_calls` entry, with `function.arguments` as compact JSON of `args`, `reasoning_content` equal to the `thought`, and `finish_reason: "tool_calls"`.
  - **WHEN** the upstream action parses to `task_complete`, **THEN** the final response **SHALL** carry `content` equal to `args.summary`, no `tool_calls`, and `finish_reason: "stop"`.
  - **WHEN** the upstream `finish_reason` is `"length"`, **THEN** the response **SHALL** carry `finish_reason: "length"`, no `tool_calls`, and no content parsed from the partial action.
  - **WHEN** a constrained upstream completion does not parse as an action, or names a tool that was not offered, **THEN** the valve **SHALL** respond with HTTP 502 and an explicit error body, and **SHALL NOT** return the raw text as an answer.
  - **WHEN** the upstream answers with a non-2xx status or cannot be reached, **THEN** the valve **SHALL** respond with HTTP 502, carrying the upstream status (or connection error class) and a bounded excerpt of the upstream error body, and **SHALL NOT** retry.
  - **WHEN** the client requested `stream: true`, **THEN** the valve **SHALL**:
    - emit a role chunk first;
    - emit `reasoning_content` deltas as the scanner yields thought text;
    - emit a `tool_calls` name delta when the scanner commits to a tool, then one arguments delta when the action completes;
    - emit `content` deltas for a final answer, then a finish chunk and `data: [DONE]`;
    - emit an empty-delta heartbeat chunk only when upstream bytes arrived since the previous downstream chunk and at least the heartbeat interval has elapsed.
  - **WHEN** the client requested `stream: false`, **THEN** the valve **SHALL** return one chat-completion object whose message equals the final message assembled in streaming mode.
  - **WHEN** the downstream client disconnects before the response completes, **THEN** the valve **SHALL** drop the upstream response so the upstream connection closes, within a bounded time that a fake upstream observes.
- **Notes:** Upstream SSE parsing handles lines split across reads. Timings and usage come from the final upstream chunk.

### T-12607: Write one content-free receipt per request, probe enforcement at startup, and provide a byte-faithful record-only mode behind a loopback CLI
- **Intent:** [INT-0012](../../../intents/INT-0012-constrained-valve-at-hermes-boundary.md)
- **Touches:** `crates/ferric-valve/src/{receipt.rs,server.rs,probe.rs,main.rs}`
- **Depends on:** T-12606
- **Acceptance criterion:** INT-0012 AC-4 (receipts), AC-6 (refuse unenforced constrained mode) and the record-only boundary.
- **Success criterion (EARS):**
  - **WHEN** a request completes, fails or is cancelled, **THEN** exactly one JSONL receipt line **SHALL** be appended, and it **SHALL NOT** contain any message text. It carries:
    - `request_id` (Hermes's correlation header when present, else generated), `mode`;
    - `tool_catalog_hash`, `schema_hash`, `prefix_hash` (over all rendered upstream messages), `message_hashes` (one SHA-256 per rendered upstream message, content-free);
    - `model`, `prompt_tokens`, `cached_tokens`, `predicted_tokens`, `prompt_ms`, `predicted_ms`;
    - `finish_reason`, `action_valid`, `tool`, `error_class`, `wall_ms`.

    A metric absent upstream **SHALL** be `null`, with `unavailable` naming it.
  - **WHEN** the valve starts without `--record-only`, **THEN** it **SHALL** send one probe request constrained to `{"type":"object","properties":{"ok":{"enum":["yes"]}},"required":["ok"],"additionalProperties":false}`, and **SHALL** exit non-zero with a diagnostic unless the reply parses to exactly `{"ok":"yes"}`.
  - **WHEN** `--record-only` is set, **THEN** every request and response body **SHALL** be forwarded byte-for-byte, streaming preserved, and receipts **SHALL** use `mode: "record-only"`.
  - **WHEN** `--listen` is omitted, **THEN** the valve **SHALL** bind `127.0.0.1` only.
- **Notes:** Constrained, pass-through and record-only share one receipt writer. Receipts are append-only with one line per request.

### T-12608: Drive real Hermes through the valve from a source-defined runner that owns and reaps every child
- **Intent:** [INT-0012](../../../intents/INT-0012-constrained-valve-at-hermes-boundary.md)
- **Touches:**
  - `crates/ferric-valve/examples/hermes_pilot.rs`, `crates/ferric-valve/e2e/hermes_driver.py`, `crates/ferric-valve/e2e/tasks/`, `docs/sprints/s126/sprint-tests/e2e-tests.md`
  - `crates/ferric-process/src/memory.rs` (new, moved), `crates/ferric-process/{Cargo.toml,src/lib.rs}`
  - `crates/ferric-cli/src/startup/memory.rs` and `crates/ferric-cli/src/startup.rs` (re-point to the moved probe)
- **Depends on:** T-12607
- **Acceptance criterion:** INT-0012 AC-7. With a pinned llama.cpp build and the reference model, one real Hermes session completes a tool call and a final answer through the valve, with receipts, a stable prefix hash across turns and a successful cancellation.
- **Success criterion (EARS):**
  - **WHEN** `--lab` resolves inside the Ferric or Amalgam repository, **THEN** the runner **SHALL** refuse to start.
  - **WHEN** the Sprint 122 system-memory probe moves from `ferric-cli` (`pub(crate)`) into `ferric-process` as a public module, **THEN** `ferric-cli`'s front-door fit behavior and its existing memory tests **SHALL** be unchanged. The move is behavior-preserving and in its own commit.
  - **WHEN** available system memory from that probe is below the model's CPU-resident estimate plus a reserve, using `ferric-core` fit, or the probe returns unknown, **THEN** the runner **SHALL** refuse to launch llama-server and record `not-run: resource gate` with the reason.
  - **WHEN** the runner starts llama-server, **THEN** it **SHALL**:
    - own the server through `ferric_process::ProcessTree`, with the exact recorded argv (context 16384, one slot, 24 GPU layers for the 27B or all layers for the 7B, flash attention, Q8 KV, no host prompt cache, `--jinja`, loopback);
    - derive every session deadline from rates measured by one warm-up request (margin × planned requests × (context ÷ prefill rate + output cap ÷ decode rate)).
  - **WHEN** a session runs, **THEN** it **SHALL**:
    - use a fresh disposable `HERMES_HOME` and fixture under the lab root;
    - erase the llama-server slot first;
    - configure Hermes with the custom provider, `agent.environment_probe: false`, `model.reasoning_echo: true`, reasoning off and the `file` toolset only;
    - run the Hermes driver through `ProcessTree` within its derived deadline.
  - **WHEN** the 7B bring-up and the 27B readiness smoke run through the constrained valve, **THEN** at least one Hermes tool call and one final answer **SHALL** complete, and within each session every constrained request's `message_hashes` list **SHALL** extend the previous constrained request's list.
  - **WHEN** the cancellation workload interrupts Hermes mid-generation, **THEN** llama-server's `/slots` **SHALL** report the slot idle within the derived bound, and the valve receipt **SHALL** be `cancelled`.
  - **WHEN** the runner exits by any path (success, error, deadline or panic unwind), **THEN** every spawned child **SHALL** be terminated and reaped through `ProcessTree`. A cleanup failure **SHALL** fail the run.
- **Notes:**
  - `hermes_driver.py` passes `ruff format --check` and `ruff check`.
  - The driver mirrors Amalgam's `evals/local_qualification/driver.py`: it imports Hermes from the Amalgam checkout via `sys.path`, uses an interrupt file and writes a result JSON.
  - No terminal toolset, so no model-authored shell.
  - The runner is an example, not a default-lane test, because it needs a GPU and Hermes. It must still compile in CI.
  - Title generation is disabled where Hermes exposes a setting; otherwise those requests are receipted as pass-through and excluded with the reason.

### T-12609: Run and report a paired native-vs-valve pilot on the 27B with independent checkers
- **Intent:** [INT-0013](../../../intents/INT-0013-constrained-decoding-on-midsize-quantized-target.md)
- **Touches:** `crates/ferric-valve/e2e/tasks/` (the four task fixtures and checkers), `crates/ferric-valve/examples/hermes_pilot.rs` (pilot plan), `docs/sprints/s126/sprint-tests/pilot/`, `docs/sprints/s126/sprint-tests/e2e-tests.md`
- **Depends on:** T-12608
- **Acceptance criterion:** INT-0013 AC-1 (a manifest pinning every coordinate) and AC-3 in pilot scope (paired, counterbalanced, three repetitions, all denominators, unsupported recorded as unsupported).
- **Success criterion (EARS):**
  - **WHEN** the pilot runs, **THEN** it **SHALL** execute 4 tasks (read-only lookup, single edit, two-file create from a directory scan, no-tool question) × 2 arms (native through `--record-only`, and valve constrained) × 3 repetitions, with arm order counterbalanced ABBA per task, on one llama-server process with identical flags.
  - **WHEN** a session ends, **THEN** its independent checker **SHALL** judge completion from fixture state or the final answer alone, never from the model's own claim.
  - **WHEN** the report is written, **THEN** `sprint-tests/pilot/report.md` **SHALL** publish, per arm:
    - checker completions over all sessions;
    - tool-call validity (JSON-object arguments and a known tool);
    - per-request prompt, cached and predicted tokens and times;
    - wall time per session and per checked completion;
    - truncations, errors, and every exclusion with its reason.
  - **WHEN** the report is written, **THEN** it **SHALL** include the manifest (model, template and llama-server hashes, the llama.cpp build, argv, the Amalgam commit, the Ferric commit, the corpus version and derived deadlines) and **SHALL** state that the pilot supports no advancement or general-capability claim. It **SHALL** also record the number of tools offered and list the Hermes-sized catalog measurement (INT-0013 AC-2) as open.
- **Notes:** Published receipts are content-free. Conversation dumps stay in the lab root, recorded by hash only. A session that hits its derived deadline counts as a failure, not an exclusion.

### T-12610: Retarget the landing docs and project instructions to the Iron role, and triage the task ledger without deleting anything
- **Intent:** [INT-0010](../../../intents/INT-0010-ferric-is-the-iron-of-amalgam.md)
- **Touches:** `README.md`, `docs/README.md`, `docs/introduction.md`, `CLAUDE.md`, `AGENTS.md`, `docs/work/tasks.md`
- **Depends on:** (none)
- **Acceptance criterion:** INT-0010 AC-1 (the landing docs state the role, the ownership split, the maintenance status and the model envelope), AC-3 (the ledger separates Iron work from maintenance with nothing deleted) and AC-5 (the project instructions state the direction).
- **Success criterion (EARS):**
  - **WHEN** `README.md`, `docs/README.md` and `docs/introduction.md` are read, **THEN** each **SHALL** name Animus Amalgam, state that Ferric supplies constrained decoding to it, state that the standalone CLI is in maintenance, and name the ~27B Q4 target class. Existing descriptions of shipped commands **SHALL** stay accurate.
  - **WHEN** `docs/work/tasks.md` is read, **THEN** every unchecked task **SHALL** either reference a live intent (INT-0010 to INT-0014) or sit under a heading containing "Maintenance", and that heading **SHALL** name the closed intent or area. The count of task lines **SHALL NOT** decrease.
  - **WHEN** `CLAUDE.md` and `AGENTS.md` are read, **THEN** each **SHALL** contain a direction section stating that Iron work comes first and standalone surfaces are maintenance-only.
- **Notes:** Docs only. The existing doc-guard tests in `crates/ferric-cli/tests/` (for example `human_docs.rs` and `source_execution.rs`) and the template-hygiene test must keep passing unchanged. Update README text around what they assert, never the guards. The new doc-content tests sit beside them.

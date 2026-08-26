Finalized - DO NOT EDIT

# Sprint 8 Build Plan — The Self-Diagnostic Testbench

Implements the launcher + diagnostic-toolbench halves of ADR-023. Multimodal
"any file" input is deferred to sprint 9. Rationale: `sprints/s8/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** the self-diagnostic testbench (`ferric server` + diagnostic `ferric toolbench`).
  - **Component A — Diagnostic toolbench**
    - T-801: Failure taxonomy — `classify()` replaces `extract_action`
    - T-802: Per-tool stats + report writer (Markdown + JSONL) + verdict
  - **Component B — `ferric server` launcher**
    - T-803: `Engine` abstraction (llama-server default, Ollama) — pure command/URL
    - T-804: `ferric server` subcommand + lifecycle + runfile
    - T-805: `query`/`toolbench` auto-discover the server runfile
  - **Component C — Docs / first-run**
    - T-806: README first-run/testbench section + PS1 drivers wrap the launcher

## Execution Sequence

### T-801: Failure taxonomy — `classify()` replaces `extract_action`
- **Touches:** `crates/ferric-cli/src/toolbench_cmd.rs`
- **Depends on:** (none)
- **Success criterion (EARS):**
  - **WHEN** a completion yields a call to the target tool with schema-valid args, **THEN** `classify` **SHALL** return `Outcome::Success`.
  - **WHEN** it yields a call to a *different* tool, **THEN** `classify` **SHALL** return `Outcome::WrongTool(name)`.
  - **WHEN** it yields the target tool but args miss a required key, **THEN** `classify` **SHALL** return `Outcome::MalformedArgs`.
  - **WHEN** nothing parseable is produced, **THEN** `classify` **SHALL** return `Outcome::NoAction`; **WHEN** action-shaped text is present but unparseable, **THEN** `Outcome::ParseError`.
- **Notes:** `enum Outcome { Success, WrongTool(String), MalformedArgs, NoAction, ParseError }` (`ProviderError` handled at the call site). Reuse `extract_action`'s protocol dispatch; add a required-keys check against the tool's `input_schema.required` (lightweight, not full JSON-Schema — a documented limit). Keep `cfg(any(feature, test))` gating so the unit tests run in default CI.

### T-802: Per-tool stats + report writer + verdict
- **Touches:** `crates/ferric-cli/src/toolbench_cmd.rs`
- **Depends on:** T-801
- **Success criterion (EARS):**
  - **WHEN** the bench finishes, **THEN** it **SHALL** print per-tool success rate + a failure histogram and an overall verdict band (≥90% solid / 70–<90% marginal / <70% unreliable).
  - **WHEN** `--report <path>` is given, **THEN** it **SHALL** write a Markdown report (the `s6/toolbench-results.md` shape + the taxonomy) AND a sibling `.jsonl` with one row per tool.
  - **WHEN** no `--report` is given, **THEN** it **SHALL** print the report to stdout and write no files.
- **Notes:** Pure `render_report(&BenchSummary) -> String` + `summary_rows(&BenchSummary) -> Vec<serde_json::Value>`, both unit-testable with no provider. `BenchSummary { model, protocol, backend, per_tool: Vec<ToolStat{name,fires,success,histogram}> }`. `verdict(rate) -> &str` is the band function.

### T-803: `Engine` abstraction (llama-server default, Ollama)
- **Touches:** new `crates/ferric-cli/src/server.rs` (+ `mod server;` in `main.rs`)
- **Depends on:** (none)
- **Success criterion (EARS):**
  - **WHEN** `Engine::LlamaServer` builds a command for `ServerConfig`, **THEN** the argv **SHALL** be `llama-server -m <model> [--mmproj <p>] -c <ctx> --host 127.0.0.1 --port <port>` (host pinned to loopback — ADR-005).
  - **WHEN** `Engine::Ollama` builds a command, **THEN** it **SHALL** invoke `ollama serve` with env `OLLAMA_HOST=127.0.0.1:<port>`.
  - **WHEN** `health_url(base_url)` is requested, **THEN** it **SHALL** return `<base>/health` for llama-server and `<base>/v1/models` for Ollama.
- **Notes:** Pure: `command(&ServerConfig) -> (program, Vec<String> args, Vec<(String,String)> env)` + `health_url`. No spawn here. `ServerConfig { engine: Engine, model: Option<String>, mmproj: Option<PathBuf>, ctx: u32, port: u16, host: String }` with `host` defaulting to `127.0.0.1`. Engine is a closed enum (never an arbitrary binary — ADR-005).

### T-804: `ferric server` subcommand + lifecycle + runfile
- **Touches:** `crates/ferric-cli/src/main.rs`, `crates/ferric-cli/src/server.rs`
- **Depends on:** T-803
- **Success criterion (EARS):**
  - **WHEN** `ferric server up` runs, **THEN** it **SHALL** spawn the engine child, poll the health endpoint until ready (bounded) or error clearly, and write `.ferric/server.json` `{engine,pid,port,base_url}`.
  - **WHEN** `ferric server status` runs with a runfile, **THEN** it **SHALL** health-check and print `base_url`; with none, **SHALL** report no server registered.
  - **WHEN** `ferric server down` runs, **THEN** it **SHALL** read the runfile, kill the PID, and remove the runfile.
  - **WHEN** `ferric server doctor` runs, **THEN** it **SHALL** report engine-binary + model presence (and, if a server is up, run the constrained capability probe).
- **Notes:** `ServerRunfile { engine, pid, port, base_url }` (serde). `up` keeps the child running past the command (store PID; don't wait). Health poll = bounded loop (reqwest, behind `backend-openai`). The spawn/kill/poll is feature-gated (needs reqwest); runfile serde + path logic stay default-testable. Real spawn = E2E heartbeat.

### T-805: `query`/`toolbench` auto-discover the server
- **Touches:** `crates/ferric-cli/src/{backend.rs,query.rs,toolbench_cmd.rs}`, `crates/ferric-cli/src/server.rs`
- **Depends on:** T-804
- **Success criterion (EARS):**
  - **WHEN** `--backend openai` is used with no explicit `--api-base` AND `.ferric/server.json` exists, **THEN** the OpenAI base_url **SHALL** default to the runfile's `base_url`.
  - **WHEN** `--api-base` is given explicitly, **THEN** it **SHALL** override the runfile (precedence: explicit > runfile > built-in default).
- **Notes:** `read_runfile(workspace) -> Option<ServerRunfile>` in `server.rs`; `backend.rs`/`query.rs` consult it. Pure precedence logic is unit-testable.

### T-806: Docs — first-run + diagnostic toolbench
- **Touches:** `README.md`, `docs/testbench.md` (new), root `run_benchmarks.ps1` / `test_both_models.ps1`
- **Depends on:** T-801, T-802, T-803, T-804, T-805
- **Success criterion (EARS):**
  - **WHEN** `README.md` is read, **THEN** it **SHALL** contain a "First run / testbench" section: `ferric server up` → `ferric toolbench --report …` → read the verdict, dialing the model down to taste.
  - **WHEN** the PS1 drivers are read, **THEN** they **SHALL** wrap `ferric server up`/`down` around `ferric toolbench` instead of assuming a manually-started server.
- **Notes:** `docs/testbench.md` holds the longer walkthrough; README links it.

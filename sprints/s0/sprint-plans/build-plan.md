Finalized - DO NOT EDIT

# Sprint 0 Build Plan — Animus Ferric Foundations

## Schema Tree
- Sprint Goal: Foundation crates for the Rust rewrite (workspace, types, scale function, trace, provider trait, security, tools, CLI, CI, remote)
  - Workspace & shared types
    - T-001: Cargo workspace scaffold
    - T-002: Core message/error types
  - Deterministic scale function
    - T-003: ModelProfile → RunPolicy tier table
  - Trajectory tracing
    - T-004: TraceEvent schema + JSONL sink + tolerant reader
  - Provider abstraction
    - T-005: Async Provider trait + MockProvider
  - Security (ferric-guard)
    - T-006: Workspace boundary
    - T-007: Permission checker + deny lists
  - Tool system
    - T-008: Tool trait + registry chokepoint
    - T-009: Builtin file tools (read_file, write_file, list_dir)
  - Surface & delivery
    - T-010: CLI stub (--version, trace cat)
    - T-011: CI workflow (win+linux + aarch64 check)
    - T-012: Record ADRs 001–009 in decisions.md
    - T-013: GitHub remote creation + push

## Execution Sequence

### T-001: Create the Cargo workspace with six empty-but-compiling `ferric-*` crates, pinned toolchain, and lint config.
- **Touches:** `Cargo.toml`, `rust-toolchain.toml`, `README.md`, `.gitignore`, `crates/ferric-{core,trace,provider,guard,tools,cli}/Cargo.toml`, `crates/*/src/lib.rs` (`main.rs` for cli)
- **Depends on:** (none)
- **Success criterion (EARS):**
  - **WHEN** `cargo build` runs at repo root, **THEN** the workspace **SHALL** compile all six crates with zero warnings.
  - **WHEN** `cargo fmt --check` runs at repo root, **THEN** it **SHALL** exit 0.
  - **WHEN** `cargo clippy --all-targets -- -D warnings` runs at repo root, **THEN** it **SHALL** exit 0.
- **Notes:** Edition 2024, stable toolchain. Dependency allowlist for s0: serde, serde_json, thiserror, async-trait (+ tempfile, futures-executor dev-only).

### T-002: Define shared vocabulary types `Message`, `Role`, `ToolCall`, `FerricError` in ferric-core.
- **Touches:** `crates/ferric-core/src/{lib.rs,message.rs,error.rs}`
- **Depends on:** T-001
- **Success criterion (EARS):**
  - **WHEN** a `Message` is serialized to JSON and deserialized, **THEN** ferric-core **SHALL** produce a value equal to the original.
  - **WHEN** `ToolCall.args` contains string, array, or object payloads, **THEN** deserialization **SHALL** succeed.
- **Notes:** Polymorphic args = Prion failure-mode #4. thiserror for FerricError.

### T-003: Implement the deterministic scale function: `ModelProfile`, `Tier`, `RunPolicy`, `Protocol`, tier table, pure `policy_for()`.
- **Touches:** `crates/ferric-core/src/scale.rs`, `crates/ferric-core/src/lib.rs`
- **Depends on:** T-002
- **Success criterion (EARS):**
  - **WHEN** `policy_for` is called twice with identical profiles, **THEN** it **SHALL** return identical `RunPolicy` values.
  - **WHEN** a profile has `params_b = 1.0`, **THEN** the function **SHALL** return the NANO policy with `protocol = ConstrainedJson`, `uses_planner = true`, and `max_tools` ≤ the NANO ceiling.
  - **WHEN** `measured_level` is `Some(l)` and contradicts the param-count tier, **THEN** the measured level **SHALL** take precedence.
- **Notes:** Tier boundaries seeded from Animus tiers.py (NANO<4B, SMALL 4–13B, MEDIUM 13–30B, LARGE, XL, ULTRA). Profiles are config-supplied, never filename-inferred (H8/H20). The measured-level override is bidirectional: a low measured level downgrades a big model, a high measured level upgrades a small one (tested both ways).

### T-004: Build ferric-trace: versioned `TraceEvent`, flush-per-event `JsonlSink`, unknown-event-tolerant `TraceReader`.
- **Touches:** `crates/ferric-trace/src/{lib.rs,event.rs,sink.rs,reader.rs}`
- **Depends on:** T-002
- **Success criterion (EARS):**
  - **WHEN** an event is written via `JsonlSink`, **THEN** the line **SHALL** be durable on disk before `write_event` returns.
  - **WHEN** the reader encounters a line whose event type is unrecognized, **THEN** it **SHALL** yield an `Unknown` variant preserving the raw JSON rather than erroring.
  - **WHEN** a `ToolResult` event is written, **THEN** the trace **SHALL** contain the full untruncated output field.
- **Notes:** Schema `{v:1, ts_ms, session, seq, event}`; s0 events: SessionStart, SessionEnd, ToolCall, ToolResult, Note. Reserve (don't define) prompt-assembly/grammar-state names for s1.

### T-005: Define the async dyn-compatible `Provider` trait with `Constraint` plumbing and a deterministic scripted `MockProvider`.
- **Touches:** `crates/ferric-provider/src/{lib.rs,traits.rs,types.rs,mock.rs}`
- **Depends on:** T-002
- **Success criterion (EARS):**
  - **WHEN** `MockProvider` is constructed with a script of N completions, **THEN** successive `complete` calls **SHALL** return them in order and the N+1th call **SHALL** return a typed `ScriptExhausted` error.
  - **WHEN** a `CompletionRequest` carries a `Constraint::JsonSchema`, **THEN** `MockProvider` **SHALL** record it retrievably.
  - **WHEN** `Provider` is used as `Box<dyn Provider>`, **THEN** the code **SHALL** compile.
- **Notes:** `Constraint = JsonSchema(Value) | Regex(String) | Lark(String)` (llguidance shapes). `Capabilities {supports_constraint, supports_native_tool_calls, exposes_logits}`. async-trait; tests via futures_executor::block_on. Streaming reserved for s1 (`ProviderEvent`).

### T-006: Implement the symlink-safe, prefix-collision-proof workspace boundary in ferric-guard.
- **Touches:** `crates/ferric-guard/src/{lib.rs,workspace.rs}`
- **Depends on:** T-002
- **Success criterion (EARS):**
  - **WHEN** `resolve` is given `../outside.txt` or an absolute path outside the root, **THEN** the Workspace **SHALL** return a `BoundaryViolation` error.
  - **WHEN** the root is `…/project` and the candidate is `…/project-evil/x`, **THEN** `resolve` **SHALL** reject it.
  - **WHEN** (on Unix) a symlink inside the workspace targets a path outside it, **THEN** `resolve` **SHALL** reject the symlinked path.
- **Notes:** Canonicalize both sides, compare `std::path::Component` sequences — handles Windows `\\?\` verbatim prefixes and case-insensitivity uniformly.

### T-007: Add the hardcoded permission checker and compile-time deny lists.
- **Touches:** `crates/ferric-guard/src/{checker.rs,denylist.rs}`
- **Depends on:** T-006
- **Success criterion (EARS):**
  - **WHEN** a Write is requested against a deny-listed path (e.g. `.git/config`, `~/.ssh/*`), **THEN** the checker **SHALL** return `Deny` with a machine-readable reason.
  - **WHEN** a Read is requested on an ordinary in-workspace file, **THEN** the checker **SHALL** return `Allow`.
  - **WHEN** deny-list contents are inspected, **THEN** they **SHALL** be compile-time constants with no runtime mutation API.
- **Notes:** `PermissionLevel {Read, Write, Execute}`. Command deny list reserved for the future exec tool. LLM never consulted.

### T-008: Build the `Tool` trait, `ToolSpec`, and registry with a single execute chokepoint.
- **Touches:** `crates/ferric-tools/src/{lib.rs,spec.rs,registry.rs}`
- **Depends on:** T-003, T-007
- **Success criterion (EARS):**
  - **WHEN** `Registry::execute` is called for a tool whose permission check denies the target, **THEN** the tool handler **SHALL NOT** run and a `Denied` result **SHALL** be returned.
  - **WHEN** a tool returns output longer than the truncation limit, **THEN** `ToolOutput` **SHALL** contain both the full output and a truncated-for-model copy.
  - **WHEN** `tools_for_policy` is called with a NANO policy, **THEN** the returned list **SHALL** have length ≤ `max_tools`.
  - **WHEN** `tools_for_policy` is called twice with the same policy and registry, **THEN** both calls **SHALL** return identical, alphabetically sorted orderings.
- **Notes:** Sync `execute` for s0; registry is the only call site so async conversion later is a one-file change (document in doc comments). Sorted enumerations = ADR-008.

### T-009: Implement builtin file tools `read_file`, `write_file`, `list_dir`, all resolving through the workspace boundary.
- **Touches:** `crates/ferric-tools/src/builtin/{mod.rs,read_file.rs,write_file.rs,list_dir.rs}`
- **Depends on:** T-008
- **Success criterion (EARS):**
  - **WHEN** `write_file` then `read_file` run on the same in-workspace path, **THEN** read **SHALL** return exactly the written content.
  - **WHEN** any of the three tools is given a path outside the workspace, **THEN** it **SHALL** fail with a boundary error and create/modify nothing.
  - **WHEN** `list_dir` runs twice on the same directory, **THEN** output ordering **SHALL** be identical (sorted).
- **Notes:** NANO-tier, JSON input schemas; full output traced, truncated copy for model.

### T-010: Build the `ferric` CLI stub: `--version` and `trace cat <file.jsonl>` derived view.
- **Touches:** `crates/ferric-cli/src/main.rs`
- **Depends on:** T-004
- **Success criterion (EARS):**
  - **WHEN** `ferric --version` runs, **THEN** it **SHALL** print the crate version and exit 0.
  - **WHEN** `ferric trace cat` is given a JSONL file containing an unknown event type, **THEN** it **SHALL** render known events and label unknown ones without crashing.
- **Notes:** std-only arg parsing; clap deferred until the CLI grows.

### T-011: Add GitHub Actions CI: fmt/clippy/test on windows+ubuntu, plus an aarch64-unknown-linux-gnu `cargo check` portability gate.
- **Touches:** `.github/workflows/ci.yml`
- **Depends on:** T-001 (sequenced after T-010 so the first run is green)
- **Success criterion (EARS):**
  - **WHEN** a push or PR lands, **THEN** CI **SHALL** run fmt, clippy, and tests on both Windows and Linux and fail on any warning.
  - **WHEN** the aarch64 check job runs, **THEN** the workspace **SHALL** type-check for `aarch64-unknown-linux-gnu`.
- **Notes:** Pi/Orange Pi/Jetson portability gate per user constraint. Run full local gate before first push.

### T-012: Record ADRs 001–009 in decisions.md and commit.
- **Touches:** `decisions.md`
- **Depends on:** T-011
- **Success criterion (EARS):**
  - **WHEN** `decisions.md` is read, **THEN** it **SHALL** contain one dated entry per ADR (001–009).
- **Notes:** ADRs: (1) workspace/harness-owns-decoding/dual-backend, FFI rejected; (2) JSONL trace source of truth, versioned, tolerant readers; (3) async dyn-compatible Provider with per-request Constraint, streaming reserved; (4) s0 dependency allowlist + aarch64 CI gate; (5) hardcoded security, component-wise boundary; (6) pure deterministic scale table, config-supplied profiles, bidirectional measured-level override; (7) edition 2024 + pinned stable + clippy -D warnings gate; (8) sorted enumerations; (9) MockProvider-only in s0, real-GGUF validation policy from s1.

### T-013: Create the public GitHub repo and push main.
- **Touches:** git remote config only (no file changes)
- **Depends on:** T-012
- **Success criterion (EARS):**
  - **WHEN** `git remote -v` runs, **THEN** origin **SHALL** point at `github.com/crussella0129/Animus_Ferric`.
  - **WHEN** the pushed main's CI run completes, **THEN** its conclusion **SHALL** be success.
- **Notes:** `gh auth status` first; verify name free; `gh repo create crussella0129/Animus_Ferric --public --source . --push`. Verify CI conclusion as a separate step before declaring done.

## Lineage-Fix Ledger (regression guard, per research risk "rewrite scope")

| Hard-won lineage fix | Source | s0 disposition |
|---|---|---|
| Workspace prefix-collision guard | Prion #1 (CRITICAL) | **T-006** ✓ |
| Symlink-escape rejection | Animus/Prion | **T-006** ✓ |
| Polymorphic tool args | Prion #4 | **T-002** ✓ |
| Sorted enumerated outputs | Prion #8 | **T-008/T-009** ✓ |
| Full-output-traced / truncated-for-model split | Animus | **T-004/T-008** ✓ |
| Hardcoded deny lists, no LLM security input | All three | **T-007** ✓ |
| Hash-ALL-calls repetition guard | Prion #5 | deferred → s1 agent loop |
| Circuit-breaker compaction | Fev | deferred → s1/s2 context manager |
| Stale-config migration (H20) | Animus | deferred → s1 config crate |
| Structured terminator (task_complete) with grammar | Animus | deferred → s1 loop + constraint wiring |
| Exponential backoff on retryable provider errors | Prion #6 | deferred → s1 real backends |
| io.LimitReader-style bounded reads | Prion #3 | deferred → s1 HTTP backend |

Deferred rows are tracked in `agent-tasks/agent-tasks.md` so they cannot silently evaporate.

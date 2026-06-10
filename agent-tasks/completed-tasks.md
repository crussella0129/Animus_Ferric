# Completed Tasks Log (Append-Only)

## T-001 (sprint 0)
- **Description:** Create the Cargo workspace with six empty-but-compiling `ferric-*` crates, pinned toolchain, and lint config.
- **Completed:** 2026-06-10T16:05:00Z
- **Files modified:** Cargo.toml, rust-toolchain.toml, README.md, .gitignore, crates/ferric-{core,trace,provider,guard,tools,cli}/Cargo.toml, crates/*/src/lib.rs, crates/ferric-cli/src/main.rs
- **Commit:** 013e0e8

## T-002 (sprint 0)
- **Description:** Define shared vocabulary types Message, Role, ToolCall, FerricError in ferric-core.
- **Completed:** 2026-06-10T16:12:00Z
- **Files modified:** crates/ferric-core/src/lib.rs, crates/ferric-core/src/message.rs, crates/ferric-core/src/error.rs
- **Commit:** 724475e

## T-003 (sprint 0)
- **Description:** Implement the deterministic scale function (ModelProfile, Tier, RunPolicy, Protocol, tier table, pure policy_for) with bidirectional measured-level override and fleet snapshot test.
- **Completed:** 2026-06-10T16:25:00Z
- **Files modified:** crates/ferric-core/src/scale.rs, crates/ferric-core/src/lib.rs, crates/ferric-core/tests/tier_table_snapshot.rs
- **Commit:** 57d23f3

## T-004 (sprint 0)
- **Description:** Build ferric-trace: versioned TraceEvent, flush-per-event JsonlSink, unknown-event-tolerant TraceReader.
- **Completed:** 2026-06-10T16:40:00Z
- **Files modified:** crates/ferric-trace/src/lib.rs, crates/ferric-trace/src/event.rs, crates/ferric-trace/src/sink.rs, crates/ferric-trace/src/reader.rs
- **Commit:** d16de53

## T-005 (sprint 0)
- **Description:** Define the async dyn-compatible Provider trait with Constraint plumbing (JsonSchema/Regex/Lark) and a deterministic scripted MockProvider that records requests.
- **Completed:** 2026-06-10T16:55:00Z
- **Files modified:** crates/ferric-provider/src/lib.rs, crates/ferric-provider/src/traits.rs, crates/ferric-provider/src/types.rs, crates/ferric-provider/src/mock.rs
- **Commit:** (see git log for T-005)

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
- **Commit:** 40dced1

## T-006 (sprint 0)
- **Description:** Implement the symlink-safe, prefix-collision-proof workspace boundary in ferric-guard (component-wise canonical containment).
- **Completed:** 2026-06-10T17:05:00Z
- **Files modified:** crates/ferric-guard/src/lib.rs, crates/ferric-guard/src/workspace.rs
- **Commit:** 0c1b6fd

## T-007 (sprint 0)
- **Description:** Add the hardcoded permission checker (Read/Write/Execute, machine-readable deny reasons) and compile-time deny lists.
- **Completed:** 2026-06-10T17:15:00Z
- **Files modified:** crates/ferric-guard/src/checker.rs, crates/ferric-guard/src/denylist.rs, crates/ferric-guard/src/lib.rs
- **Commit:** 382ff6b

## T-008 (sprint 0)
- **Description:** Build the Tool trait, ToolSpec, and registry with a single execute chokepoint (pre-handler guard check, timing, full/truncated output split, sorted+capped tools_for_policy).
- **Completed:** 2026-06-10T17:30:00Z
- **Files modified:** crates/ferric-tools/src/lib.rs, crates/ferric-tools/src/spec.rs, crates/ferric-tools/src/registry.rs
- **Commit:** (see git log for T-008)

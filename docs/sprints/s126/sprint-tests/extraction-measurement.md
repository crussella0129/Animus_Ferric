# Sprint 126 — extraction measurement and Stage B gate (T-12602)

The owner asked that the extraction approach be tried and measured before
anything else. This record is that measurement, and it decides whether
Stage B (the valve) may start.

## What moved (T-12601, commit `1128b77`)

| Destination in `crates/ferric-iron/src/` | Lines | Moved from |
|---|---|---|
| `types.rs` | 84 | `ferric-provider/src/types.rs` (`Capabilities`, `ToolDescriptor`, `Constraint`, `StreamDelta`, and one serde test) |
| `stream_scan.rs` | 498 | `ferric-provider/src/stream_scan.rs` (verbatim) |
| `grammar.rs` | 189 | `ferric-loop/src/grammar.rs` (`action_schema`, `parse_json_action`, `ActionParseError`, 8 tests) |
| `terminator.rs` | 244 | `ferric-loop/src/terminator.rs` (verbatim) |
| `protocol.rs` | 112 | `ferric-loop/src/protocol.rs` (verbatim) |
| `render.rs` | 60 | new: `render_tool_listing` and `tool_result_text`, extracted from `ferric-loop/src/run.rs` and `projector.rs` |
| `lib.rs` | 25 | new |

In total, 1,112 lines were removed from `ferric-loop` and `ferric-provider`
and 33 were added there (re-export shims and the two call sites). The old
modules remain as shims, so no import path in the workspace changed. The XML
fallback parser stays in `ferric-loop`, because it needs `regex`.

## Dependency boundary (INT-0011 AC-1)

`cargo tree -e normal`, unique crates including the crate itself:

| Crate | Unique crates |
|---|---:|
| `ferric-iron` | **19** (ferric-core, serde, serde_json, thiserror, and their proc-macro helpers) |
| `ferric-provider` | 28 |
| `ferric-loop` | 147 |

`tests/dependency_boundary.rs` pins the boundary, so shipped dependencies must
be a subset of {ferric-core, serde, serde_json, thiserror}, and ferric-core's
must be a subset of {serde, serde_json, thiserror}.

Mutation control: adding `regex` to `ferric-iron`'s `[dependencies]` failed
the test with this message:

> crates/ferric-iron/Cargo.toml ships dependencies outside the
> constrained-decoding core boundary: ["regex"]

Reverting restored 2/2 passing.

## Clean build time

Each crate was built into a fresh `CARGO_TARGET_DIR` with `cargo build -p <crate>
--locked`, debug profile, on a 24-core host with sources already downloaded. The
builds ran one after the other.

| Crate | Clean build |
|---|---:|
| `ferric-iron` | **6.1 s** |
| `ferric-loop` | 25.2 s |

## Behavior preservation (INT-0011 AC-2)

The canonical command was `cargo test --workspace --locked -- --test-threads=1`
on Windows.

| Run | Passed | Failed | Ignored | Suites |
|---|---:|---:|---:|---:|
| Baseline at `13a1832` (code identical to `bf3c7d8`; only docs differ) | 1,326 | 0 | 13 | 82 |
| After the move | 1,329 | 0 | 13 | 85 |

The +3 passes are exactly the three tests T-12601 added:

- `tool_listing_bytes_match_legacy_format`
- `tool_result_text_matches_legacy_format`
- `reexport_paths_resolve`

The +3 suites are `ferric-iron`'s unit and doc suites and the new
`iron_reexports` integration test.

Per-crate accounting shows every moved test was kept:

- `ferric-loop` went from 131 to 114 (−17);
- `ferric-provider` went from 47 to 37 (−10);
- `ferric-iron` has 29, which is the 27 moved tests plus the 2 render tests.

No test was deleted, and the ignored count is unchanged.

The locked EARS clause says the totals "SHALL equal the baseline". Read
literally, the added tests break that equality. The clause exists to prove
that no test was lost or broken by the move, and the accounting above proves
that directly. This deviation is recorded here rather than hidden.

## Gate decision

**Pass.** The boundary holds with an effective mutation control. The core
compiles in a quarter of `ferric-loop`'s time, with one eighth as many crates.
All pre-existing tests pass unchanged. Stage B (the valve on `ferric-iron`)
may start, and no alternative (copying code, a Python middleware) is needed.

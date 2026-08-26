# Sprint 103 research — C7, and what is actually wrong with `ferric-cli`

## The logged claim

> **C7 — split `ferric-cli`.** Still 9,674 lines across 19 flat modules;
> `mcp.rs`, `query.rs`, `toolbench_cmd.rs` are the bulk. The shared spine is
> correctly factored; the command modules want subdirectories.

## Measured

8,396 lines across 19 modules (the 9,674 figure counted tests too).

| module | lines | |
|---|---:|---|
| query.rs | 1,344 | ~410 of it is the `QueryArgs` clap struct |
| mcp.rs | 1,197 | JSON-RPC types + a ~370-line `McpServer` impl |
| toolbench_cmd.rs | 960 | pure report/classify helpers + one driver |
| server.rs | 835 | |
| chat.rs | 770 | |
| …14 more | ≤457 each | |

## The finding: subdirectories are not the problem

Long is not the same as tangled, and the audit's own sentence concedes the
spine is fine. Moving 19 files into folders changes no behaviour, finds no
defect, and costs a large diff across every `git blame` in the crate. On its
own it is the lowest-value item in the backlog.

**But there is a real defect of the kind this crate keeps producing: one
concept implemented three times.**

`chat.rs` and `icm.rs` each declare their own backend enum, and the
declarations are identical apart from the name:

```rust
enum ChatBackend {  /* chat.rs:207 */          enum Backend {  /* icm.rs:203 */
    Mock,                                          Mock,
    #[cfg(feature = "backend-openai")]             #[cfg(feature = "backend-openai")]
    Real { provider: Box<dyn Provider + Send + Sync>, runtime: Runtime },
}                                              }
```

Their constructors are line-for-line the same — same `Runtime::new()`, same
`map_err` wording, same `block_on(create_provider)` — and their
`cfg(not(feature))` stubs have **byte-identical** error strings
(`chat.rs:323/330`, `icm.rs:448/453`).

`mcp.rs` is the same idea taken apart: `Executor` (`mcp.rs:253`) holds only the
runtime, with the provider carried beside it in `McpServer`, and
`build_real_provider` (`mcp.rs:650`) returns the pair — a third construction of
"a provider plus the thing that drives it", with a third stub carrying the same
message again.

So the crate has three spellings of *resolved backend*, all built the same way,
all failing the same way when the feature is off. This is the pattern the last
four sprints each hit from a different angle: the protocol key in six places
(ADR-089), the trace directory in six (ADR-090), the skill parser in two
(ADR-091), the trace walk in two (ADR-093).

## Why it matters beyond tidiness

The stubs are what a user meets when the binary is built without
`backend-openai` — the exact condition that produced sprint 101's false
positive, where a feature-less rebuild looked like a security gate working.
Three copies of that path is three places for the diagnostic to drift, and
nothing tests any of them.

## Non-findings, recorded so they are not re-investigated

- **`toolbench_cmd.rs` deliberately does not read back the persisted profile**
  (`measured_level: None`, `max_ring` from the flag alone, line 477). That is
  correct for a calibrator — it must not consume the profile it is about to
  write — and must not be "unified" with `query.rs`'s read-back.
- **`dream_cmd.rs`'s stub message differs on purpose**: it has no `--mock`, so
  telling the user to use one would be wrong.
- **The shared spine really is factored.** `backend.rs::create_provider` is
  already the single provider constructor; the duplication sits one layer
  above it, in who owns the runtime.

## Scope call

Do the unification; do **not** do the subdirectory move. This report says
plainly that the second half of C7 is being declined and why, rather than
shipping churn and calling the item closed.

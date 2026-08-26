# Sprint 103 build plan — Finalized - DO NOT EDIT

**Goal:** `ferric-cli` stops spelling "a resolved backend" three times.
Behaviour-neutral throughout.

## T-10301 — one backend type, one builder, one diagnostic

In `backend.rs`:

1. `pub(crate) const BACKEND_FEATURE_MISSING: &str` — the "built without
   backend features; rebuild … or use --mock" text, currently byte-identical in
   three stubs. One definition.
2. `create_provider_with_runtime(opts) -> Result<(Provider, Runtime), String>`
   — the `Runtime::new()` + `block_on(create_provider)` pair written three
   times.
3. `pub(crate) enum ResolvedBackend { Mock, Real { provider, runtime } }` plus
   `ResolvedBackend::real(opts)`, replacing `chat::ChatBackend` and
   `icm::Backend`, which are identical declarations.
4. Delete the dead `#[cfg(not(feature = "backend-openai"))]` branch inside
   `create_provider` — the whole function is already gated on that feature, so
   the branch cannot compile in and its error string is unreachable. It also
   contradicts the comment directly below, which says callers own that case.

Then `chat.rs` and `icm.rs` drop their enums and builders and use the shared
one; `mcp.rs` keeps its own `Executor` but builds through (1) and (2).

## Deliberately NOT done, with reasons

- **`mcp::Executor` is not merged into `ResolvedBackend`.** `McpServer` builds
  its provider **once at launch and reuses it for every `tools/call`** (the
  documented T-3601 shape), while chat and icm construct a fresh mock per
  invocation. `MockProvider` is *scripted and stateful*, so when it is
  constructed is behaviour, not style — collapsing the shapes would silently
  change one of them. Only the construction is shared; the lifetime difference
  stays and is now commented.
- **The subdirectory move (the rest of C7) is declined.** The audit's own
  sentence concedes the spine is factored; moving 19 files changes no
  behaviour, finds nothing, and rewrites every `git blame` in the crate. The
  backlog entry is closed with that stated, not quietly marked done.
- **`toolbench_cmd.rs`'s non-read-back of the persisted profile is correct**
  for a calibrator and is left alone (see the research report).

## T-10302 — record

ADR-094, backlog C7 closed with what was and was not done, completed-tasks,
README timeline entry.

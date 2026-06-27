Finalized - DO NOT EDIT

# Sprint 29 Build Plan — `apply_patch`: round out Ring 2

Add the second Ring-2 tool (the "room to grow" named in the rings memory): `apply_patch`
applies a context-located unified diff to one file, atomically. Distinct from `multi_edit`:
context **disambiguates** which occurrence to edit (multi_edit only hits the first), and the
diff format is model-familiar. Rationale: `sprints/s29/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** `apply_patch` shipped as a Ring-2 builtin, registered + tested + recorded.
  - **A. the tool** — T-2901
  - **B. registration + ring-gate** — T-2902
  - **C. ADR + docs** — T-2903

## Execution Sequence

### T-2901: `ApplyPatch` builtin (Ring 2)
- **Touches:** `crates/ferric-tools/src/builtin/apply_patch.rs` (new)
- **Depends on:** —
- **Description:** `ApplyPatch` impl `Tool`, mirroring `multi_edit.rs` — `ToolSpec{name:"apply_patch", description, input_schema:{path, patch}, permission: Write, ring: 2}`. `run`: resolve path, read working copy, `split('\n')`; parse hunks (a hunk starts `@@`, header ignored; lines ` `/`-`/`+); per hunk build `before=context+removed`, `after=context+added`; splice the first contiguous `before` run → `after`; after all hunks succeed, rejoin + write **once**. Any failure → `Err`, no write.
- **Success (EARS):**
  - WHEN every hunk's `before` block is located THEN it SHALL write once and report success.
  - WHEN any hunk is unlocatable or the patch is malformed/empty THEN it SHALL `Err` and leave the file byte-identical.
  - WHEN a hunk's context uniquely pins the Nth occurrence THEN it SHALL edit that occurrence.

### T-2902: Register + ring-gate
- **Touches:** `crates/ferric-tools/src/builtin/mod.rs`
- **Depends on:** T-2901
- **Description:** `mod apply_patch;` + `pub use apply_patch::ApplyPatch;` + `registry.register(Box::new(ApplyPatch))`.
- **Success (EARS):** WHEN a Medium policy is resolved THEN `tools_for_policy` SHALL include `apply_patch`; WHEN Nano/Small THEN it SHALL NOT.

### T-2903: ADR-039 + docs
- **Touches:** `decisions.md`, `README.md`, `agent-tasks/agent-tasks.md`, `agent-tasks/completed-tasks.md`
- **Depends on:** T-2902
- **Description:** ADR-039 (rounds out Ring 2; context-located line-based atomic hunks; distinct-from-multi_edit rationale; single-file scope, multi-file deferred). README Status 29 + Sprint 29 timeline.
- **Success (EARS):** WHEN the sprint closes THEN `decisions.md` SHALL contain ADR-039 and README SHALL show Sprint 29.

## Post-build (test)
- `cargo test -p ferric-tools` (new `apply_patch` tests + updated `rings_gate` Medium 12) + `cargo test --workspace` green; clippy `-D warnings`; fmt.

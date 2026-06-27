# Sprint 29 Research Report — `apply_patch`: round out Ring 2 (the room to grow)

## Sprint goal (in my words)
The loop-hardening guard family is complete (s22/27/28). Pivot back to the **tool rings**
(the project's north star). Ring 2 ("plan & apply structured changes") was *seeded* with
`multi_edit` (s19) and proven drivable (qwen-7b calibrates `--max-ring 2` at 100%), but it's
a single tool. This sprint adds the second Ring-2 tool the rings memory + backlog name as
"the room to grow": **`apply_patch`** — apply a context-located unified diff to one file,
atomically.

**Why `apply_patch` over the existing `multi_edit`** (the justification — they must not be
redundant):
1. **Context-based disambiguation.** `multi_edit` replaces the **first** occurrence of each
   `old_string` (`replacen(_, _, 1)`) — it *cannot* target the 3rd occurrence of a common
   line. A diff hunk carries surrounding **context** lines, so it locates a *specific* site.
2. **Diff-format familiarity.** Models are heavily trained on unified diffs (git); expressing
   a multi-line change as a hunk is often more natural than reproducing an exact `old_string`.

## Decisions Reviewed
- **ADR-028 (+ amendments, s18/19)** — the ring system (`ToolSpec.ring`, `ring_for_tier`,
  `tools_for_policy` trims outer-ring-first). `apply_patch` is `ring: 2`, like `multi_edit`.
  This sprint extends ADR-028's Ring 2; no revision.
- **ADR-013/permission model** — `apply_patch` is `PermissionLevel::Write` (mutates a file),
  so it flows through the existing guard/permission-check chokepoint unchanged.

## Existing Code Survey
| File | Role / relevance |
|---|---|
| `crates/ferric-tools/src/builtin/multi_edit.rs` | The Ring-2 sibling to mirror: `Tool::spec()` → `ToolSpec{name, description, input_schema, permission: Write, ring: 2}`; `run(ctx, args)` resolves the path, builds an in-memory **working copy**, validates **all** edits before a **single** atomic `std::fs::write` (a failure leaves the file byte-identical). `apply_patch` uses the same all-or-nothing shape. |
| `crates/ferric-tools/src/builtin/mod.rs` | `register_builtin_tools` (add `ApplyPatch`); `pub use`; `path_arg` helper (reused). |
| `crates/ferric-tools/tests/builtin_file_tools.rs` | `rings_gate_builtins_by_tier`: Nano 6 / Small 10 / **Medium 11** → becomes **12** (Ring 0 + Ring 1 + `multi_edit` + `apply_patch`); the integration-test harness (`setup()` → temp ws + registry) to mirror for `apply_patch` tests. |
| `crates/ferric-tools/src/registry.rs` | `tools_for_policy` already ring-gates by tier; no change. Medium `max_tools` = 16 (scale.rs) ≥ 12, so no trimming. |
| `crates/ferric-tools/src/spec.rs` | `Tool`/`ToolSpec`/`ToolCtx` contracts. |
| `crates/ferric-core/src/scale.rs` | Medium tier `max_tools = 16`; confirms a 12th tool fits at Medium without the cap dropping anything. |

## External Sources
None — internal tool-rings work; the unified-diff hunk format (` `/`-`/`+` line prefixes) is
common knowledge, and `apply_patch` deliberately uses **context matching, not `@@` line
numbers**, so no external spec dependency.

## Risks / unknowns / dependencies
- **Newline / line-boundary fragility.** Unified diffs are line-oriented; naive substring
  matching mishandles trailing newlines. **Mitigation:** apply **line-based** — split the
  working copy on `\n`, locate the contiguous run of lines equal to the hunk's
  `context+removed` block, replace with `context+added`, rejoin on `\n`. No line-number
  reliance; matches actual file content.
- **Hunk not locatable / ambiguous.** If a hunk's `before` block isn't found → error, **no
  write** (atomic, like `multi_edit`). If it appears multiple times, apply at the **first**
  match in the current working copy (context usually makes it unique; documented).
- **Malformed patch.** Empty patch, no hunks, a line without a ` `/`-`/`+ prefix → a clear
  `Err(...)`. Covered by tests.
- **Scope:** **single-file** (the `path` arg names the target; the patch body is that file's
  hunks) — mirrors `multi_edit`'s single-file model. Multi-file patches are noted as future work.
- **Ring-count test** is the one existing test that must change (Medium 11→12); additive.

## Recommended approach
A new `crates/ferric-tools/src/builtin/apply_patch.rs` (`ApplyPatch`, `ring: 2`,
`PermissionLevel::Write`):
- **Args:** `{ "path": string, "patch": string }`. `patch` is a unified-diff body: one or
  more hunks; a hunk starts at a line beginning `@@` (the header text is **ignored** — we
  match by context), then lines prefixed ` ` (context), `-` (remove), `+` (add).
- **Apply (line-based, atomic):** parse hunks; for each, build `before = context+removed`
  and `after = context+added` (as line vectors); locate the first contiguous `before` run in
  the working line-vector and splice in `after`; after all hunks succeed, rejoin and write
  **once**. Any failure → `Err`, no write.
- **Register** in `mod.rs`; bump the rings-gate test to Medium 12.
- **Tests** (mirror `multi_edit`'s): a single hunk changes the right line; **context
  disambiguates** (a hunk targets the 2nd of two identical lines where `multi_edit` would hit
  the 1st — the defining contrast); a hunk whose context isn't found → error **and the file
  is byte-identical**; an empty/garbled patch → error; a multi-hunk patch applies in order;
  `rings_gate` Medium == 12 incl. `apply_patch`.

### Alternative considered — multi-file `apply_patch` (deferred)
A diff spanning several files (create/update/delete) is more powerful but needs all-or-
nothing semantics *across* files (resolve + validate all, then write all, with rollback on a
late failure) — materially more complex. Single-file first matches `multi_edit` and keeps the
sprint tight + well-tested; multi-file is a clean follow-on once single-file is proven.

### Alternative considered — L7+ bench levels (deferred)
Extending the bench ladder above L6 was the other candidate. Rejected for now: the fleet
*tops out* at L6 (qwen-7b), so an L7 would be a level **every** current model fails — it
discriminates nothing until a stronger model is in scope. `apply_patch` delivers immediate,
testable capability; L7+ is better paired with a larger model.

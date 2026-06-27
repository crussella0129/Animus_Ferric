# Plan Critique — Sprint 29

> Self-critique against `prompts/plan-critic.md` (no subagent spawn — autonomous loop).

## Concerns

### C-001: Is `apply_patch` redundant with `multi_edit`?
- **Failure mode:** duplicate-capability
- **Response (reject):** no — and the plan makes the distinction a *tested* assertion. `multi_edit`
  replaces the **first** occurrence only (`replacen(_,_,1)`); it cannot target the 2nd of two
  identical lines. `apply_patch` locates a hunk by its **context**, so it disambiguates. The
  "defining contrast" test edits the second of two identical lines — provably impossible with
  `multi_edit`. Plus diff-format familiarity. Genuinely additive.

### C-002: Unified-diff parsing is notoriously fiddly (line numbers, newlines)
- **Failure mode:** fragile-parser
- **Response (fix-in-plan, reflected):** we **ignore** `@@` line numbers entirely and match by
  context, and apply **line-based** (split on `\n`, locate a contiguous line run, splice,
  rejoin) — sidestepping the substring/trailing-newline pitfalls. The format handled is the
  robust common subset (` `/`-`/`+ prefixed lines). Malformed input → a clear `Err`, tested.

### C-003: Atomicity / partial writes
- **Failure mode:** corruption-on-failure
- **Response:** identical guarantee to `multi_edit` — all hunks validated+applied to an
  in-memory working copy; the file is written **once** only if every hunk located. A
  "unlocatable hunk → byte-identical file" test asserts no partial write.

### C-004: Ambiguous hunk (context matches multiple sites)
- **Failure mode:** wrong-edit-site
- **Response:** apply at the **first** match in the current working copy (deterministic) and
  document it; in practice a well-formed hunk includes enough context to be unique. This mirrors
  `multi_edit`'s first-occurrence determinism — not a regression, and the disambiguation win is
  precisely that *more* context narrows the match.

### C-005: Scope creep to multi-file patches
- **Failure mode:** over-scope
- **Response:** explicitly **single-file** (the `path` arg names the target). Multi-file
  all-or-nothing is deferred to a follow-on — keeps this sprint tight and well-tested.

### C-006: The ring-count test must change
- **Failure mode:** brittle-test-churn
- **Response:** expected + intended — `rings_gate_builtins_by_tier` Medium 11→12 is the one
  additive assertion change; Nano/Small unchanged. Medium `max_tools=16` ≥ 12 so the cap drops
  nothing.

## Confidence
`clean` — a small, additive, pure-`std::fs` builtin mirroring a proven sibling (`multi_edit`),
with a tested capability that `multi_edit` provably lacks (context disambiguation), atomic
write semantics, and the one expected ring-count test bump. No cross-cutting risk.

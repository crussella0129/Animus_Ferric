# Plan Critique — Sprint 12

> Self-critique against `prompts/plan-critic.md`.

## Concerns

### C-001: Substring vs regex
- **Failure mode:** scope / under-power
- **Response:** **accept (deliberate).** ferric-tools has no `regex` dep; a literal substring is dependency-free (ADR-004), ReDoS-free, and covers the dominant "find this identifier" case. Regex is a clean, isolated follow-on if the need shows up — not worth a new dep + ReDoS surface in the first increment.

### C-002: Security — a new tool that walks the tree
- **Failure mode:** boundary-escape
- **Response:** **covered by design + test.** Every path resolves through `ctx.workspace.resolve` (ADR-005); `target_paths` returns the search root so the registry permission-checks it before `run`; the recursion only descends within the resolved (in-bounds) root. A boundary-refusal test is in the test plan. Zero new permission (Read only).

### C-003: Output flooding a small model
- **Failure mode:** budget blowout (ADR-018)
- **Response:** **bounded.** `max_results` cap (default 50) stops the walk; the registry truncates further for the model. Per-line content is the matched line only.

### C-004: Determinism of a filesystem walk
- **Failure mode:** non-deterministic output (ADR-008)
- **Response:** **handled.** Directory entries are sorted before descent, so walk order — and thus the capped result set — is stable; a determinism test asserts it.

## Confidence
`clean` — one additive, guard-scoped tool on the established `list_dir` pattern; fully AI-verifiable through the registry chokepoint; no new deps, no security delta.

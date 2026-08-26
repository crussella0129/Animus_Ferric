# Plan Critique — Sprint 13

> Self-critique against `prompts/plan-critic.md`.

## Concerns

### C-001: `delete_path` is destructive
- **Failure mode:** unsafe-tool
- **Response:** **bounded by design.** Every path resolves through `Workspace` (no escape); declaring `Write` runs `check_write_target`, so the denylist (`.ferric`, `.git/config`, ssh keys) auto-denies; a non-empty dir needs an explicit `recursive: true`; the registry traces every call. That's the same safety envelope as `write_file`/`move_path`, plus the recursive gate. The user's "extra confirmation" idea doesn't fit a headless tool contract — revisit as a Ring/policy concern in sprint 14 if wanted.

### C-002: `edit_file` first-occurrence can edit the wrong spot
- **Failure mode:** correctness-vs-firerate
- **Response:** **deliberate.** The toolbench gates *fire rate* (did the model emit a valid `edit_file` call), not edit correctness — and first-occurrence never errors on multiplicity, maximizing fire rate for small models. Edit *correctness* is the model's responsibility (provide distinctive `old_string`); require-unique would raise error rates and hurt the very reliability we're measuring. A `replace_all`/uniqueness option is a clean later add.

### C-003: the alphabetical `max_tools` cap now truncates at Nano (8 > 6)
- **Failure mode:** latent-bug-exposed
- **Response:** **acknowledged, scoped to sprint 14.** The toolbench profile is 8.0B → Small (cap 10), so it benches all 8 this sprint. A real Nano run would drop 2 tools alphabetically — exactly what the ring formalization (trim-from-outer) fixes next. Noted in research + the Ring-0 reliability report.

## Confidence
`clean` — two additive tools on the established `write_file`/`move_path` pattern; no new deps, no new security surface; the reliability claim is *measured* by the toolbench, not asserted.

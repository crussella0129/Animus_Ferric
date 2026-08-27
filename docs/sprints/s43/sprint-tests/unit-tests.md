# Sprint 43 Unit Tests

All derived from the locked `build-plan.md` EARS clauses. All green.

## T-4302 — `animus-launch` co-located `#[cfg(test)]` (`src/lib.rs`)
- `validate_project_name_rejects_empty` / `validate_goal_rejects_empty`: empty + whitespace-only →
  `Err`; a reasonable value → `Ok`.
- `derive_initial_tasks_nonempty`: a multi-clause goal → a non-empty `Vec<String>` where each clause
  becomes a task referencing the goal's content; a separator-free goal → exactly one task
  (not a fixed boilerplate list).
- `target_clobber_safety_rules` (the safety core): the `target_is_clobber_safe` predicate is
  asserted across all four cases — absent path → safe; empty dir → safe; dir with a hidden-only
  entry (`.keep`) → NOT safe; an existing FILE (not a dir) → NOT safe (plan-critic C-004).

## T-4303 — `ferric-cli` `launch.rs` co-located `#[cfg(test)]`
- `spec_from_answers_builds_valid_spec`: valid name/path/goal → `Ok(LaunchSpec)` with trimmed
  fields; an optional `project_type` passes through (trimmed, empty → `None`).
- `spec_from_answers_rejects_invalid`: empty name, empty goal, or empty path → `Err` (no scaffold
  attempted).

## Result
`cargo test -p animus-launch` (lib): 4 passed. `cargo test -p ferric-cli` (bin unit): 72 passed
(up from 70 — +2 launch). `--features backend-openai`/`backend-mistralrs`: unaffected.

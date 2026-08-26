Finalized - DO NOT EDIT

# Sprint 43 Build Plan

## Schema Tree
- Sprint Goal: Animus Launch increment 1 — `ferric launch` (deterministic scaffolder + interview)
  - Decision record
    - T-4301: ADR-053 — Animus Launch (posture + placement)
  - The core
    - T-4302: `animus-launch` crate — `LaunchSpec` + validators + `scaffold()`
  - The surface
    - T-4303: `ferric launch` subcommand + interview + wire into `main.rs`
  - Tests
    - T-4304: unit + integration (scaffold tempdir, refuse-to-clobber, CLI subprocess)
  - Docs
    - T-4305: README + main.rs surface doc + agent-tasks wrap-up

## Execution Sequence

### T-4301: ADR-053 — Animus Launch (posture + placement)
- **Touches:** `decisions.md`
- **Depends on:** (none)
- Records: Launch's distinct security posture (user-run, deterministic, LLM-free — it CREATES a
  workspace, so `ferric-guard` containment N/A; the real property is **refuse-to-clobber**); git as
  a named subprocess boundary (ADR-013, like `ferric server` → `llama-server`); the `animus-launch`
  library crate + `ferric launch` subcommand placement (mirroring `ferric-research` ↔ `ferric-cli`);
  the scaffold shape (git `main`+`dev` + sprint-loop skeleton) + the fixed-identity scaffold-commit
  decision (so CI-without-git-identity works); the hand-rolled-stdin interview (no new dependency);
  explicit deferrals (GECK-style profile library, the "begin work?" Loop auto-hand-off, environment
  detection, richer goal→task NLP).
- **Success criterion (EARS):**
  - **WHEN** ADR-053 is read, **THEN** it **SHALL** state Launch's distinct (non-agent, LLM-free,
    refuse-to-clobber) posture, the git-subprocess boundary, the crate/subcommand placement, and the
    explicit deferrals.

### T-4302: `animus-launch` crate — `LaunchSpec` + validators + `scaffold()`
- **Touches:** new `crates/animus-launch/{Cargo.toml,src/lib.rs}`; workspace `Cargo.toml` (add to
  `members` + `[workspace.dependencies]`)
- **Depends on:** T-4301
- `LaunchSpec { name, path, goal, project_type }`; pure `validate_project_name`/`validate_goal`
  (echoing GECK's validators) → `Result<(), String>`; pure `derive_initial_tasks(goal) ->
  Vec<String>`; `scaffold(&LaunchSpec) -> Result<ScaffoldReport, LaunchError>`:
  1. **Refuse-to-clobber (plan-critic C-004 — the sole safety property, defined precisely):** safe
     iff `!path.exists()` OR (`path.is_dir()` AND `read_dir(path).next().is_none()`). Hidden
     entries (`.git`, `.DS_Store`) COUNT as non-empty (the stricter, safer rule — never
     silently scaffold over an existing `.git`). A path that exists but is NOT a directory (file or
     symlink) → `LaunchError::TargetNotEmpty` (never scaffold over it). Else → `TargetNotEmpty`.
  2. Validate name + goal → `LaunchError::Invalid`.
  3. `create_dir_all(&path)` AND `create_dir_all(path.join("agent-tasks"))` (plan-critic C-001 —
     `fs::write` does not create parents; the nested `agent-tasks/` dir must exist first).
  4. Write the seed skeleton: `README.md` (title = name, body = goal), `.gitignore` (the sprint-loop
     block — `sprints/`, `target/`, `*.tmp`), `agent-tasks/agent-tasks.md` (the goal-derived tasks),
     `decisions.md` (`# Architectural Decisions` header).
  5. Run git as a sequence of `Command::output()` calls (plan-critic C-002 — NOT `server.rs`'s
     spawn/status shape; each step must complete, be checked, and capture stderr): `git init` →
     `git add -A` → `git -c user.name="Animus Launch" -c user.email="launch@animus.local" commit -m
     "Initial scaffold (Animus Launch)"` → `git branch -M main` (rename the default branch → `main`,
     portable across git versions — the canonical form; the research report's `git init -b main` is
     superseded, C-003) → `git branch dev`. **After each command, check `output.status.success()`;
     on failure return `LaunchError::Git(String)` populated from stderr** (so a failed `commit`
     never silently proceeds to `branch`).
  6. Return `ScaffoldReport { path, files_created, branches: ["main", "dev"] }`.
- `LaunchError { TargetNotEmpty(PathBuf), Invalid(String), Git(String), Io(#[from])}`. Deps:
  `thiserror` (already allowlisted — C-007); dev-dep `tempfile` (already allowlisted). Both via
  `{ workspace = true }` like `ferric-research`. Co-located `#[cfg(test)]` for the pure helpers.
- **Success criterion (EARS):**
  - **WHEN** `validate_project_name`/`validate_goal` get invalid input (empty name / empty goal),
    **THEN** they **SHALL** return `Err`.
  - **WHEN** `derive_initial_tasks(goal)` is called with a non-empty goal, **THEN** it **SHALL**
    return a non-empty `Vec<String>` of seed backlog bullets derived from the goal.
  - **WHEN** `scaffold` targets a path that already exists and is non-empty, **THEN** it **SHALL**
    return `LaunchError::TargetNotEmpty` and create/modify nothing.
  - **WHEN** `scaffold` targets a valid empty/absent path, **THEN** it **SHALL** create a git repo
    with a `main` branch AND a `dev` branch, an initial commit, and the four seed skeleton files.

### T-4303: `ferric launch` subcommand + interview + wire into `main.rs`
- **Touches:** new `crates/ferric-cli/src/launch.rs`; `crates/ferric-cli/src/main.rs`;
  `crates/ferric-cli/Cargo.toml` (dep `animus-launch`)
- **Depends on:** T-4302
- `LaunchArgs { name, path, goal, project_type }` (all `Option`). Pure `spec_from_answers(...) ->
  Result<LaunchSpec, String>`. Thin `prompt_line(question) -> io::Result<String>` (hand-rolled plain
  stdin, no new dep). `run_launch`: for each MISSING field, prompt in the fixed order **name → path
  → goal** (a field supplied by flag is skipped); build the spec via `spec_from_answers`; call
  `animus_launch::scaffold`; print the `ScaffoldReport` to **stdout** (or error + `ExitCode::FAILURE`).
  **Interview prompts print to STDERR** (plan-critic C-006 — so stdout stays the report only, and a
  piped-answer test can assert on stdout; the fixed prompt order lets the test supply exactly the
  missing fields positionally). `Command::Launch(Box<LaunchArgs>)` wired into `main.rs`.
- **Success criterion (EARS):**
  - **WHEN** `ferric launch --name N --path P --goal G` is run (all fields supplied), **THEN** it
    **SHALL** scaffold non-interactively (no prompt) and report the created repo.
  - **WHEN** a required field is missing, **THEN** `run_launch` **SHALL** prompt for it on stdin and
    build the spec from the answers.
  - **WHEN** `spec_from_answers` gets an invalid name/goal, **THEN** it **SHALL** return `Err` (no
    scaffold attempted).

### T-4304: tests
- **Touches:** `crates/animus-launch` (integration `tests/` or co-located), `crates/ferric-cli/src/launch.rs`
  (unit), `crates/ferric-cli/tests/cli.rs`
- **Depends on:** T-4303
- animus-launch: unit (validators, `derive_initial_tasks`) + integration (`scaffold` → a
  `tempfile::tempdir()`: assert `.git/` exists, `git branch` lists `main`+`dev`, the four seed files
  exist with expected content; refuse-to-clobber on a non-empty dir returns `TargetNotEmpty` and
  touches nothing). ferric-cli: `spec_from_answers` unit arms; CLI subprocess — `ferric launch
  --name --path --goal` (non-interactive) into a tempdir asserts the repo; a piped-stdin run
  (interactive, reusing the sprint-42 `run_chat_mock`-style harness) asserts the repo from answers.
- **Success criterion (EARS):**
  - **WHEN** the unit + integration + CLI tests run, **THEN** every T-4302/T-4303 EARS clause
    **SHALL** have a passing test, including the refuse-to-clobber and the `main`+`dev`-branch
    assertions.

### T-4305: docs
- **Touches:** `README.md`, `crates/ferric-cli/src/main.rs` (surface doc), `agent-tasks/agent-tasks.md`,
  `agent-tasks/completed-tasks.md`
- **Depends on:** T-4301–T-4304
- README Status bump + Sprint 43 timeline entry; `main.rs` surface doc adds `ferric launch`; the
  sprint-43 backlog section rewritten in-progress → completed summary (sprints 38–42 precedent); the
  Animus-Launch suite pillar marked started (inc 1).
- **Success criterion (EARS):**
  - **WHEN** README's Sprint 43 entry + `main.rs`'s surface doc are read, **THEN** both **SHALL**
    describe `ferric launch` (scaffolder + interview, inc 1) with an ADR-053 reference.

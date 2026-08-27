Finalized - DO NOT EDIT

# Sprint 43 Test Plan

## Unit Tests
### T-4302 (`animus-launch`)
- `validate_project_name_rejects_empty` / `validate_goal_rejects_empty`: empty (and whitespace-only)
  → `Err`; a reasonable name/goal → `Ok`.
- `derive_initial_tasks_nonempty`: a real goal → a non-empty `Vec<String>`; each bullet references
  the goal's content (not a fixed boilerplate list).
### T-4303 (`ferric-cli` `launch.rs`)
- `spec_from_answers_builds_valid_spec`: valid name/path/goal → `Ok(LaunchSpec)` with those fields.
- `spec_from_answers_rejects_invalid`: empty name or empty goal → `Err` (no scaffold attempted).

## Integration Tests
### T-4304 (`animus-launch` — `scaffold` against a temp dir; needs `git` on PATH)
- `scaffold_creates_git_repo_with_main_and_dev`: `scaffold` into a `tempfile::tempdir()` subpath →
  `.git/` exists; `git rev-parse --verify main` AND `git rev-parse --verify dev` both succeed; the
  HEAD commit exists; the four seed files (`README.md`, `.gitignore`, `agent-tasks/agent-tasks.md`,
  `decisions.md`) exist. **Seed-content assertions use a substring/normalized check** (README
  CONTAINS the goal; `.gitignore` CONTAINS `sprints/`; agent-tasks CONTAINS a derived task) rather
  than exact bytes — so Windows `core.autocrlf` line-ending rewrites can't flake the test
  (plan-critic C-008; the suite runs on ubuntu AND windows-latest).
- `scaffold_refuses_to_clobber_nonempty_dir`: a dir already containing a file → `TargetNotEmpty`,
  the pre-existing file untouched, no `.git` created.
- `scaffold_refuses_to_clobber_hidden_only_dir` (plan-critic C-004): a dir containing ONLY a hidden
  entry (e.g. a `.keep` file) → `TargetNotEmpty` (hidden entries count as non-empty).
- `scaffold_refuses_to_clobber_existing_file` (plan-critic C-004): a target path that already exists
  as a FILE (not a dir) → `TargetNotEmpty`, the file untouched.
- `scaffold_commit_works_without_global_git_identity`: the fixed `-c user.name`/`-c user.email`
  identity makes the initial commit succeed regardless of the ambient git identity (proven by the
  HEAD commit existing in the temp repo).

### T-4304 (`ferric-cli` — `ferric launch` subprocess)
Uses a NEW stdin-piping helper modeled on the sprint-42 `run_chat_mock` SHAPE (`Stdio::piped` +
`write_all` + `wait_with_output`) — not the literal function, which hardcodes `chat --mock` and
asserts exit-success (plan-critic C-005).
- `launch_noninteractive_scaffolds`: `ferric launch --name demo --path <tmp>/demo --goal "a tiny
  CLI"` (no stdin) → exit success; `<tmp>/demo/.git` exists, `main`+`dev` present, seed files
  present.
- `launch_interactive_scaffolds_from_stdin`: `ferric launch --path <tmp>/demo2` with the missing
  fields (name, then goal) piped on stdin in that fixed order → the repo is scaffolded from the
  answers; the `ScaffoldReport` on stdout names the created path (prompts went to stderr, C-006).

## Regression
Every existing test unaffected — `launch` is a new subcommand + a new crate; `query`/`mcp`/`chat`/
`trace`/`server`/`bench` and all existing crates are untouched.

## End-to-End Tests
- **Status:** possible — the `ferric launch` subprocess tests ARE the end-to-end proof (a real
  binary scaffolds a real git repo on disk), filed under Integration (sprints 38–42 precedent). No
  separate live-model dependency exists (Launch is LLM-free).
- Manual smoke: `ferric launch --name demo --path /tmp/demo --goal "x"` then `git -C /tmp/demo log
  --oneline && git -C /tmp/demo branch`.

## Build/Lint (all tasks)
`cargo test --workspace` green (requires `git` on PATH — GitHub Actions has it; the aarch64 gate is
type-check-only and unaffected); `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt
--all --check`; `--features backend-openai`/`--features backend-mistralrs` builds unaffected.

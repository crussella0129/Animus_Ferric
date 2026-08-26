# Sprint 43 Integration Tests

Real git-on-disk + real-binary subprocess tests (need `git` on PATH — GitHub Actions has it; the
aarch64 CI gate is `cargo check --workspace`, type-check only, so these never run there). All green.

## T-4304 — `animus-launch/tests/scaffold.rs` (`scaffold` against a temp dir)
- `scaffold_creates_git_repo_with_main_and_dev`: `scaffold` into a `tempfile::tempdir()` subpath →
  `.git/` is a dir; `git rev-parse --verify main` AND `... dev` both succeed; HEAD is on `main` with
  the "Initial scaffold" commit; the four seed files exist, content asserted by SUBSTRING (README
  carries the goal, `.gitignore` has `sprints/`, agent-tasks has a derived task + `- [ ]`) —
  Windows-`autocrlf`-safe (plan-critic C-008).
- `scaffold_refuses_to_clobber_nonempty_dir` / `_hidden_only_dir` / `_existing_file` (all 3 edges,
  plan-critic C-004): each returns `LaunchError::TargetNotEmpty`, touches nothing, creates no `.git`.
- `scaffold_commit_works_without_global_git_identity`: the fixed `-c` identity makes exactly one
  commit exist in the temp repo regardless of the ambient git identity.

## T-4304 — `ferric-cli/tests/cli.rs` (`ferric launch` subprocess)
A NEW `run_launch` stdin-piping helper modeled on `run_chat_mock`'s SHAPE (not the literal fn —
plan-critic C-005).
- `launch_noninteractive_scaffolds`: `ferric launch --name --path --goal` (no stdin) → success;
  `.git` + README + agent-tasks present; `git branch` lists `main` + `dev`.
- `launch_interactive_scaffolds_from_stdin`: `ferric launch --path <p>` with name + goal piped (in
  that fixed order) → success; the `ScaffoldReport` on stdout names the project; README carries the
  piped name + goal (prompts went to stderr, C-006).
- `launch_refuses_to_clobber_nonempty`: `ferric launch` at a dir with an existing file → non-zero
  exit, the file untouched, no `.git` — the safety property proven at the CLI boundary.

## Regression
Every existing test unaffected — `launch` is a new subcommand + a new crate; all prior crates and
CLI surfaces are untouched.

## Result
`cargo test -p animus-launch --test scaffold`: 5 passed. `cargo test -p ferric-cli --test cli`: 35
passed (up from 32 — +3 launch). `cargo test --workspace`: all green. `cargo clippy --workspace
--all-targets -- -D warnings`: clean (default + both backend feature sets). `cargo fmt --all
--check`: clean.

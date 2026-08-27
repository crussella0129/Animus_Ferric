# Sprint 43 E2E Tests

- **Status:** possible — the `ferric launch` subprocess tests (`integration-tests.md`) ARE the
  end-to-end proof: a real `ferric` binary scaffolds a real git repo on disk (main+dev, seed
  skeleton, an actual commit) from flags or piped stdin. Launch is **LLM-free**, so there is no
  live-model dependency at all — the `--mock`/live-backend split that gates other subcommands
  doesn't exist here; the subprocess tests exercise the true production path directly.
- Filed under Integration (T-4304) rather than duplicated here, per sprints 38–42's precedent.
- Manual smoke (already run during the build): `ferric launch --name demo --path /tmp/demo --goal
  "a tiny CLI"` → `git -C /tmp/demo log --oneline` shows the scaffold commit on `main`, `git -C
  /tmp/demo branch` shows `main` + `dev`, and the seed files (README with the goal, `agent-tasks/`
  with derived tasks, `.gitignore`, `decisions.md`) are present.
- **Deferred (inc 2+):** the "begin work?" hand-off that actually launches a first sprint against
  the new repo — inc 1 prints the hand-off hint but does not auto-invoke the Loop; that end-to-end
  Launch→Loop chain is the natural next increment.

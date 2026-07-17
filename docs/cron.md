# Agentic Cron — scheduled periodic agent tasks

`ferric cron` schedules Ferric's own operations to run periodically. The
motivating case: consolidate recent traces into memory **every 12 hours**
(`/dream every 12h`) without remembering to run it by hand.

## Jobs are files

Each job is one TOML file under `<workspace>/.ferric/cron/`. The filename stem is
the job name.

```toml
# .ferric/cron/nightly.toml
schedule = "12h"          # interval: 30s / 15m / 12h / 2d, or hourly/daily/weekly
command  = "dream"        # a Ferric subcommand: "dream" or "query"
enabled  = true           # optional (default true)
```

A `query` job carries a prompt (and may run offline against the mock):

```toml
# .ferric/cron/summary.toml
schedule = "daily"
command  = "query"
prompt   = "Summarize the changes since the last run into NOTES.md."
mock     = false          # true = run against the offline mock (for testing)
enabled  = true
```

## Commands

```sh
ferric cron add nightly --schedule 12h --command dream
ferric cron add summary --schedule daily --command query --prompt "…"

ferric cron list                 # jobs, schedules, last-run and next-due
ferric cron run                  # run every currently-due job once, then exit
ferric cron run --dry-run        # report which jobs are due, run nothing
ferric cron watch                # loop: run due jobs each tick until Ctrl-C
ferric cron watch --interval 5m  # check every 5 minutes (default 60s)
```

All commands take `--workspace <path>` (default: current directory); the cron
directory is always `<workspace>/.ferric/cron/`.

`cron run` is a single tick — run it from an external scheduler, or use
`cron watch` as a standalone foreground daemon (background it with your shell if
you want it detached).

## What a job may run — the security boundary

A job's `command` is **not** an arbitrary shell string. It is one of a fixed set
of Ferric subcommands (`dream`, `query`), so every scheduled action is an
operation Ferric already contains: a `query` runs the workspace-scoped,
guard-checked agent loop; `dream` reads traces and writes `MEMORY.md`. This is
deliberately narrower than the [hooks](../decisions.md) system (arbitrary scripts
the user writes per workspace) — a scheduled, standing trigger should not be able
to run arbitrary commands. New capabilities extend the command set, never open it
to a shell.

## Scheduling semantics

- Schedules are **recurrence intervals**, not calendar/crontab expressions. A job
  is due when a full interval has elapsed since its last run (a never-run job is
  due immediately).
- Last-run timestamps live in `.ferric/cron/.state.json` — a runtime cache kept
  out of the user-authored job files. A missing or corrupt state file is treated
  as "nothing has run yet".
- State advances on **attempt**, not success: a job that fails still records its
  run time, so it reschedules to the next interval instead of firing every tick.

## Deferred

Calendar/crontab expressions (e.g. "weekdays at 09:00"); a detached watcher daemon
with a runfile and lifecycle management; misfire/catch-up policy for a watcher that
was down across a due window; more job command kinds (e.g. an ICM pipeline run) as
the set grows.

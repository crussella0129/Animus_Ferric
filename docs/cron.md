# Agentic Cron — scheduled periodic agent tasks

`ferric cron` schedules Ferric's own operations to run periodically. The
motivating case: consolidate recent traces into memory **every 12 hours**
(`/dream every 12h`) without remembering to run it by hand.

## Jobs are files

Each job is one TOML file under `<workspace>/.ferric/cron/`. The filename stem is
the job name.

```toml
# .ferric/cron/nightly.toml
schedule = "12h"          # interval, OR a cron expression (see below)
command  = "dream"        # a Ferric subcommand: "dream" or "query"
enabled  = true           # optional (default true)
```

A `schedule` is either a **recurrence interval** or a **cron expression**:

- **Interval** — `30s` / `15m` / `12h` / `2d`, or the aliases `hourly` / `daily` /
  `weekly`. Due once a full interval has elapsed since the last run.
- **Cron expression** — a standard 5-field expression `minute hour day-of-month
  month day-of-week`, **evaluated in UTC**. Fields support `*`, a number, a range
  (`1-5`), a list (`1,3,5`), and a step (`*/15`). Day-of-week is `0-6` (0 =
  Sunday; `7` also means Sunday). Examples:

  | Expression | Fires |
  |---|---|
  | `0 2 * * *` | every day at 02:00 UTC |
  | `0 9 * * 1-5` | 09:00 UTC on weekdays (Mon–Fri) |
  | `*/15 * * * *` | every 15 minutes |
  | `0 0,12 1 * *` | midnight & noon on the 1st of each month |

  As in Vixie cron, when **both** day-of-month and day-of-week are restricted, the
  job fires when **either** matches.

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
directory is always `<workspace>/.ferric/cron/`. Workspace paths are resolved
to canonical absolute paths before spawning child job subprocesses (`query`/`dream`),
guaranteeing clean execution regardless of whether relative paths (e.g. `--workspace ./app`) are provided.

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

- An **interval** job is due when a full interval has elapsed since its last run
  (a never-run job is due immediately). A **cron** job is due when the current UTC
  minute matches its expression and it has not already fired during that minute.
- Last-run timestamps live in `.ferric/cron/.state.json` — a runtime cache kept
  out of the user-authored job files. A missing or corrupt state file is treated
  as "nothing has run yet".
- State advances on **attempt**, not success: a job that fails still records its
  run time, so it reschedules instead of firing every tick.
- Cron expressions are UTC to keep due-computation a deterministic function of the
  clock (no dependence on the host timezone). Run `ferric cron watch` often enough
  that a matching minute is not skipped — the default 60s tick catches every
  minute.

## Deferred

Local-timezone cron expressions (UTC-only today); a detached watcher daemon with a
runfile and lifecycle management; misfire/catch-up policy for a watcher that was
down across a due window; more job command kinds (e.g. an ICM pipeline run) as the
set grows.

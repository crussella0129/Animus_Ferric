# CLAUDE.md — Animus Ferric

Project-specific instructions. These take precedence over the global
`~/.claude/CLAUDE.md`.

## Direction: Ferric is the Iron of Animus Amalgam

Since 2026-09-26 ([INT-0010](docs/intents/INT-0010-ferric-is-the-iron-of-amalgam.md)),
Ferric supplies harness-owned constrained decoding to Animus Amalgam, the
owner's fork of Hermes Agent.

- **Iron work first.** That means the constrained-decoding core
  (`crates/ferric-iron`), the OpenAI-compatible valve Hermes points at
  (`crates/ferric-valve`), and component-level evidence (INT-0011 to
  INT-0014).
- **Standalone surfaces are maintenance-only.** This covers the human front
  door, server lifecycle and Tailscale, ICM, cron, Ornstein, skills, MCP,
  Animus Launch and the autonomy runner. They get fixes, not features.
  Retiring any of them needs its own owner-approved intent.
- **Amalgam's Book is Amalgam's.** Hermes integration and session-level
  qualification belong to its own Book. Do not edit it from a Ferric sprint.

## Branching: two branches, and only two

**This repository has exactly two branches: `main` and `dev`.** Set by the
owner on 2026-07-27, after ~20 accumulated `sprint/*` branches were deleted.

- **Work in `dev`.** Every sprint's commits land on `dev` directly.
- **Do NOT create a branch per sprint.** No `sprint/NNN-*` branches, no
  worktrees for sprints, no feature branches. This is explicit, not a default
  to be reasoned around.
- **One PR per sprint, `dev` → `main`,** opened as the final step of the
  sprint's loop phase.
- **The owner approves and performs every merge.** Never merge a PR.
- After a merge, bring `dev` back in step with `main` before starting the next
  sprint, so the next PR contains exactly one sprint.

### Why this replaced the previous flow

The old flow branched `sprint/NNN-*` off the previous sprint's branch. That
produced a stack of long-lived branches, and stacked PRs behaved badly:
**GitHub only retargets a PR's base when the base branch is deleted**, not when
it is merged, so PRs #57/#58 merged into each other and `main` received only
the bottom sprint. Recovering it needed an extra "land sprints N and N+1" PR.

A single `dev` branch removes the stack entirely: there is one base, it is
always `main`, and a PR can only ever contain what is on `dev`.

### The one thing to check before opening a sprint PR

`git log origin/main..dev` must contain **only the current sprint's commits**.
If it spans more than one sprint, the previous sprint's PR was missed — fix the
cadence, do not bundle. (See the `one-pr-per-sprint` memory for the full
close-out order: commit → push and CONFIRM → PR → verify the commit count.)

## Sprint records

Sprint Loops Book v2 is tracked under `docs/`. `docs/intents/` owns durable
semantic intent, `docs/work/tasks.md` and `docs/work/completed-tasks.md` own
work state, and `docs/sprints/` owns sprint provenance. `docs/SUMMARY.md` is a
navigation view only; `docs/history/` preserves migrated legacy records and is
not authoritative. Do not recreate the retired root-level sprint ledgers.

## Template hygiene

This repo is meant to be usable as a template (ADR-096). Machine identity —
real tailnet addresses, MagicDNS suffixes, hostnames, account handles, concrete
home directories, LAN IPs — must not enter tracked sources.
`crates/ferric-cli/tests/template_hygiene.rs` enforces this and will fail the
suite. Use documentation values instead: `tailnet-example.ts.net`,
`100.64.0.x`, `example-host`, `C:\Users\<you>`.

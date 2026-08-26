# Sprint 105 research — detaching the repo from the machine that grew it

**Goal (user, 2026-07-26):** make Ferric a ready-to-go template — "as little of
my local paths as possible in the code itself."

## The repo already learned this lesson once

Sprint 97 (ADR-088) found a **tracked** `docker/.env` pinning `MODELS_PATH` to
one machine's NAS drive. Because it was tracked, it overrode the compose default
on *every* checkout, and `docker compose up` failed anywhere that drive was not
mapped. The fix was the standard pattern: untrack the real file, ship a
`.env.example`, gitignore the original.

**That lesson was applied to exactly one file.** This sprint applies it
everywhere else it holds.

## Inventory

### A. Tracked files the project's own tooling calls ephemeral

- **`sprints/` — 139 tracked files, and `sprints/` is in `.gitignore`.**
  `.gitignore` does not untrack what is already tracked, so the ignore rule
  added around sprint 33 silently did nothing for the existing files. The
  ignore block states the intent outright: *"Ephemeral sprint working memory —
  regenerable; the real outcome lives in the per-task git commits +
  decisions.md."* A template ships 139 files its own tooling says are
  regenerable — including `s0`'s surveys, which carry personal notes.
- **`scratch/parse_trace.py`** — a one-off scratch script.
- **`benchmarks/results.jsonl`** (34 rows) — this machine's benchmark run log.

### B. Machine identity baked into Rust test fixtures

These are the "code itself" the goal names, and they are the sharpest ones —
not paths but **identity**:

| file | leak |
|---|---|
| `ferric-cli/src/server.rs:732-751` | real tailnet IP `100.86.207.71`, real MagicDNS suffix `tail944782.ts.net`, real hostname `TEC-XX`, real node ID |
| `ferric-research/src/retriever.rs:707-708` | real tailnet IPs + device names + **the owner's account handle** (`crussella0129@`) |
| `ferric-research/src/retriever.rs:654,677,720` | real device names as fixture hosts |
| `animus-launch/src/lib.rs:202` | a comment citing `C:\Users\charl\Cargo.toml` |

The comments say these are captured from real output, which is why they are
faithful — and faithful is what the tests need. **The shape is what the tests
assert; the identity is incidental.** Every one of them can keep its shape with
documentation-range values.

*Not a leak:* `ferric-cli/src/config.rs:256-265` uses `/home/x/.config`, which
is already a synthetic placeholder. Left alone.

### C. Machine-specific defaults in scripts and config

- `tools/run_benchmarks.ps1:4` — default `D:\Models\gguf\Llama-3.2-1B-…gguf`.
  A default nobody else can satisfy.
- `docker/.env.example:5` — the commented example is `Y:\Models\gguf`, this
  machine's NAS drive. An example should teach the shape, not one drive letter.
- `docker/docker-compose.yml:47` — `${MODELS_PATH:-../../Animus/Models}`
  assumes the checkout sits beside an `Animus/` directory. That is the suite
  layout, not this machine — a softer case, kept but documented.

### D. The one with *behavioural* consequences: `benchmarks/model_profiles.json`

Tracked, and read back at runtime by ADR-029: a stored `measured_level`
overrides the parameter-count prior **in both directions**. It currently ships
`qwen2.5-coder-3b → 4/Small` and `qwen2.5-coder-7b → 6/Large`, measured here.

A fresh user running the same model *name* silently inherits a tier this
project measured on someone else's setup, at a quantization the record does not
pin. ADR-029 established that a profile **miss** is a safe no-op; nothing
considered what a profile **hit from another machine** means. For a project
whose thesis is *measure, don't assume*, shipping unearned measurements as
defaults is the wrong direction — so the same `.example` treatment applies.

## Explicitly out of scope, and why

**`decisions.md`, `agent-tasks/`, and the git history are not scrubbed.** The
ADRs are the project's reasoning record, and rewriting them to remove the
machine that produced the measurements would falsify the evidence they cite.

But that is a *choice*, and it has a consequence worth stating rather than
burying: **`decisions.md` contains tailnet IPs and device names, and untracking
files does not remove them from git history.** If this template is ever
published, that history goes with it. Removing it needs a history rewrite — a
destructive, force-push operation on a shared branch, which is the owner's call
and not something to do unasked. Recorded as a decision for the user, not a
task.

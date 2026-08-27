# Sprint 43 Research — Animus Launch (the GECK-successor bootstrapper), increment 1

## Decisions Reviewed
- **ADR-005** (security is hardcoded/harness-owned; workspace containment; the LLM is never
  consulted on a security decision): **Animus Launch has a genuinely DIFFERENT security posture,
  and that's the key architectural point.** Launch is a **user-run, deterministic, LLM-free**
  scaffolder that CREATES a new project workspace at an arbitrary path — it is not a workspace-
  scoped agent operation. `ferric-guard`'s containment (which confines an *agent* to one workspace)
  doesn't apply and shouldn't be forced onto it. The safety property Launch needs is different:
  **refuse to clobber** (never scaffold into a non-empty dir / existing repo). This distinction
  wants its own ADR.
- **ADR-011** (command structure; no chat catch-all): a new `ferric launch` subcommand slots in
  additively (like `mcp`/`chat` did) — the CLI-first, one-binary design. Launch is inherently
  interactive (an interview), but the deterministic *scaffolding core* can be a non-interactive,
  fully-testable function with the interview as a thin UX layer on top (mirroring how Ornstein built
  the testable primitive first, the wizard later).
- **ADR-013** (ownership-graph boundaries are named, not absolute): Launch shells out to `git`
  (`git init`, branch, commit) — a named subprocess boundary, exactly like `ferric server` shells
  out to `llama-server`. No new *library* dependency; `git` is invoked as a closed set of
  subcommands (never an arbitrary command), preserving the auditability posture.
- **[[animus-suite-direction]]** (memory): Animus Launch is a named suite pillar — "interview you
  about goals → scaffold a git repo with main+dev already established → ends by asking 'begin
  work?' (handing off to the Loop). Decision: lives as a CRATE in Animus_Ferric."

## Sprint goal (own words)
Start Animus Launch — the interactive project bootstrapper (GECK's successor) — as a new crate in
the Ferric monorepo. GECK (`~/GECK`, Python-only; the memory's "partial Rust geck-cli" is stale —
there is no Rust there) is a *macro-prompt/memory scaffolder*: an interview (`interactive.py`)
collects name/path/goal/project-type, and a generator (`generator.py` + Tera-like templates) writes
a `GECK/` folder (LLM_init/env/tasks/log) with goal-derived initial tasks and project-type
*profiles*. **What GECK does NOT do — and what makes Launch distinct — is bootstrap a real git repo
(main+dev) and hand off to an agent loop.** Animus Launch = GECK's "interview → scaffold" adapted to
Ferric's own sprint-loop structure (`agent-tasks/`, `decisions.md`, a gitignored `sprints/`) + git
bootstrapping + the Loop hand-off.

## Existing Code Survey
| File | Relevance |
| --- | --- |
| `~/GECK/geck_generator/cli/interactive.py` | The reference interview flow (questionary): project_name → local_path → goal → repo_url → project-type profile, with validators. The UX Animus Launch's interview (a later increment) mirrors — but in Rust. |
| `~/GECK/geck_generator/core/generator.py` | `init_geck_folder()` — the deterministic scaffold: creates a folder, renders templates from config, derives initial tasks from the goal (`_derive_initial_tasks`/`_parse_goal_to_bullets`), detects the environment. The model for Launch's `scaffold()`. **Confirms GECK does no git init/branches** — its `subprocess` calls are only version-detection. |
| `~/GECK/geck_generator/core/profiles.py` | `PROFILES` — project-type templates (web/apps/data/automation/libs/games/systems) that pre-fill config (frameworks/platforms/success-criteria). A `--type` for Launch could echo a small subset. |
| `crates/ferric-cli/src/server.rs` | The subprocess precedent (ADR-013): `ferric server` shells out to `llama-server` via a closed enum, std-only, host-pinned. Launch's `git` invocation follows the same shape (a closed set of subcommands, `std::process::Command`, no shell). |
| `crates/ferric-cli/src/main.rs` | The `Command` enum — a `Launch(Box<LaunchArgs>)` variant slots in exactly as `Chat`/`Mcp` did. |
| `Cargo.toml` (workspace) | The 10-crate workspace; a new `animus-launch` library crate is added here (holding the scaffolding logic, pure/testable), driven by a `ferric launch` subcommand — mirroring `ferric-research` (Ornstein logic) ↔ `ferric-cli` (CLI surface). |
| `agent-tasks/agent-tasks.md`, `decisions.md`, `.gitignore` (this repo) | The sprint-loop scaffold shape a new Launch-created repo should be seeded with, so it's immediately Loop-ready (the whole point of the hand-off). |

(7 files — within the 20-file research budget.)

## External Sources
None fetched — the reference (GECK) is a local repo, read directly. This is an internal
architecture + product-scope decision, not an external-practice question.

## Risks, unknowns, dependencies
1. **Scope: Launch is a big new subsystem** (like Ornstein). Increment 1 must be a tight, testable
   slice, not the whole bootstrapper. The interactive interview (questionary-equivalent) is hard to
   unit-test and is UX, not core — a natural inc-2 deferral.
2. **Git-on-disk testing** — `scaffold()` does real `git init`/branch/commit into a temp dir. Fully
   testable (scaffold into `tempfile::tempdir()`, assert the `.git`, the `main`+`dev` refs, the
   seeded files) but requires `git` on PATH in CI (GitHub Actions has it; the aarch64 gate is
   type-check-only, unaffected).
3. **Clobber safety** — Launch must refuse to scaffold into a non-empty directory / existing repo
   (the one real safety property; no LLM is involved, so no agentic risk). A pure precondition check.
4. **Naming/placement** — `ferric launch` subcommand + an `animus-launch` library crate is the
   natural fit ("lives as a crate", monorepo); whether it eventually gets its own `animus`-branded
   binary is a later call, not forced now.

## Recommended approach (+ alternative considered)
**Recommended increment-1 scope: the deterministic scaffolding CORE + a non-interactive CLI.**
- New library crate `animus-launch`: `LaunchSpec { name, path, goal, project_type }` +
  `scaffold(&LaunchSpec) -> Result<ScaffoldReport, LaunchError>` that deterministically (no LLM):
  (a) precondition-checks the target path is safe (doesn't exist / is empty — refuse to clobber);
  (b) creates the dir; (c) `git init` + writes a seed skeleton (README from the goal, `.gitignore`
  with the sprint-loop block, `agent-tasks/agent-tasks.md` with goal-derived initial tasks echoing
  GECK's `_derive_initial_tasks`, an empty `decisions.md`); (d) initial commit on `main`; (e) create
  the `dev` branch. Pure enough to unit-test end-to-end against a temp dir.
- A `ferric launch --name <n> --path <p> --goal <g> [--type <t>]` subcommand driving it.
- **ADR-053** documenting Launch's distinct posture (user-run deterministic scaffolder, no LLM,
  refuse-to-clobber, git-as-a-named-subprocess-boundary) and the crate/subcommand placement.
- **Deferred to inc 2+:** the interactive interview (questionary-equivalent), the full GECK-style
  project-type profile library, the "begin work?" Loop hand-off (auto-invoking a first sprint),
  environment detection, and richer goal→task derivation.

**Alternative considered: lead with the interactive interview** (port `interactive.py`'s wizard
first). Rejected as inc-1 primary: interactive stdin is hard to unit-test, it's UX rather than the
load-bearing scaffolding logic, and the project's own precedent (Ornstein's testable primitive
before the wizard; chat mode's parse/logic split from the REPL) is to build the deterministic core
first. The interview is a thin layer that sits cleanly on top of a done `scaffold()` next increment.

## Scope Decided (user, 2026-07-09, after research)
The user chose **"Both this sprint"** — build the deterministic scaffolder AND the interactive
interview together in increment 1. Locked scope for the Plan Phase:
- **`animus-launch` library crate:** `LaunchSpec { name, path, goal, project_type }`; pure
  validators (echoing GECK's `validate_project_name`/`validate_goal`); `scaffold(&LaunchSpec) ->
  Result<ScaffoldReport, LaunchError>` — deterministic, LLM-free: refuse-to-clobber precondition →
  create dir → `git init -b main` → seed skeleton (README from goal, `.gitignore` with the
  sprint-loop block, `agent-tasks/agent-tasks.md` with goal-derived tasks, `decisions.md`) → initial
  commit **with `-c user.name/-c user.email` fallbacks** (so it works in CI where git may have no
  global identity) → create the `dev` branch. Fully unit/integration tested against a temp dir.
- **`ferric launch` subcommand:** non-interactive when `--name`/`--path`/`--goal` are supplied;
  otherwise a **hand-rolled plain-stdin interview** (no new dependency — matches the conservative
  allowlist and the sprint-42 chat REPL precedent) collecting the missing fields. The answer→spec
  logic is a PURE function (`spec_from_answers`), unit-tested; the stdin loop is a thin layer,
  integration-tested via a piped subprocess (like the chat tests).
- **ADR-053** documents the posture + placement.
- **Still deferred to inc 2+:** the full GECK-style project-type profile *library* (inc 1 has a small
  fixed `--type` set or none), the "begin work?" auto-hand-off that actually launches a first sprint,
  environment detection, and richer goal→task NLP. Confirmed via `AskUserQuestion`.

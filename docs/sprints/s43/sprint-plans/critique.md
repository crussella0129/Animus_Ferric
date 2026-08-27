# Plan Critique — Sprint 43

Reviewed by a foreground plan-critic that traced the git-bootstrap sequence in three scenarios and
verified the aarch64-gate interaction against real source. **Both highest-value checks pass:** the
`git init` → commit → `git branch -M main` → `git branch dev` sequence correctly yields a repo on
`main` with `dev` present regardless of `init.defaultBranch` (verified with `-c
init.defaultBranch=master`), and the aarch64 CI job is `cargo check --workspace` (type-check only,
ci.yml:52) so scaffold's git-subprocess tests never run there; `thiserror`+std are aarch64-clean. 8
concerns, all fixable-in-plan tightenings — no architectural rejection.

## C-001: `agent-tasks/` nested dir needs an explicit `create_dir_all` (would fail at runtime)
- **Finding:** T-4302 lists one "create dir" step then writes four files, one nested
  (`agent-tasks/agent-tasks.md`). `std::fs::write` doesn't create parents → `NotFound` at runtime.
- **Response:** **fix-in-plan.** T-4302 now states: `create_dir_all` the target root AND
  `agent-tasks/` before writing (each file's parent is ensured first).

## C-002: `LaunchError::Git` — each git step must check `status.success()` + capture stderr
- **Finding:** the plan cited `server.rs` as the subprocess precedent, but `server.rs` uses
  fire-and-forget `.spawn()` / `.status()` with output nulled — it never captures stderr into a
  typed error. `scaffold` needs the opposite (run to completion, check exit, capture stderr), or a
  failed `git commit` would silently proceed to `git branch`.
- **Response:** **fix-in-plan.** T-4302 now specifies: run each git subcommand via
  `Command::output()`, check `output.status.success()` between steps, and populate
  `LaunchError::Git(String)` from stderr on any failure. The "mirrors server.rs" framing is
  corrected to "server.rs is the closed-subcommand-set precedent; the capture-and-check error
  handling is new."

## C-003: git-init form inconsistent (research `-b main` vs build-plan `git init` + `branch -M main`)
- **Finding:** the research report said `git init -b main` (needs git ≥2.28); the build-plan says
  the more portable `git init` + `git branch -M main` (no version floor).
- **Response:** **fix-in-plan.** The build-plan's portable form (`git init` + `git branch -M main`
  after the commit) is canonical; a note in T-4302 flags the research report's `-b` phrasing as
  superseded.

## C-004: refuse-to-clobber "empty" is underspecified (the sole safety property)
- **Finding:** "non-empty dir" doesn't define hidden-only dirs, a path that's a FILE not a dir, or a
  symlink. Only the plain non-empty-dir case was tested.
- **Response:** **fix-in-plan.** T-4302 now defines the precondition precisely: **safe iff
  `!exists()` OR (`is_dir()` AND `read_dir().next().is_none()`)** — hidden entries (`.git`,
  `.DS_Store`) COUNT as non-empty (the stricter, safer rule); a path that exists but is not a
  directory → `LaunchError::TargetNotEmpty` (never scaffold over a file/symlink). T-4304 adds tests
  for the file-not-dir and hidden-file-only cases.

## C-005: the interactive test needs a NEW helper of `run_chat_mock`'s shape, not the literal fn
- **Finding:** `run_chat_mock` hardcodes `chat --mock` and asserts exit-success; it can't be pointed
  at `ferric launch`.
- **Response:** **fix-in-plan.** Reworded to "a new stdin-piping helper modeled on `run_chat_mock`'s
  SHAPE (`Stdio::piped` + `write_all` + `wait_with_output`)" — the pattern is reused, not the
  function.

## C-006: prompt destination (stderr) + fixed prompt order — the piped test depends on both
- **Finding:** prompts must go to stderr (so stdout stays the `ScaffoldReport` the test asserts on),
  and the missing-field prompt order must be deterministic (name, path, goal) so piped answers line
  up — neither was stated.
- **Response:** **fix-in-plan.** T-4303 now specifies: interview prompts print to **stderr** (stdout
  = the report only); the prompt order is fixed **name → path → goal** (a field supplied by flag is
  skipped, and the piped test supplies exactly the missing fields in that order).

## C-007: ADR-004 allowlist — no amendment needed (verified, positive)
- **Finding:** `thiserror = "2"` and `tempfile = "3"` are both already in `[workspace.dependencies]`;
  `animus-launch` uses `{ workspace = true }` like `ferric-research`. No amendment; aarch64-clean.
- **Response:** **reject as a concern** (no action) — the dependency claim holds; recorded so it
  isn't re-litigated.

## C-008: scope realism for "both" — Windows CI git behavior is the likely surprise
- **Finding:** the 5-task "both" decomposition under-costs the interview's prompt-only-if-missing
  logic and cross-platform git behavior (tests run on ubuntu AND windows-latest, ci.yml:17;
  `core.autocrlf` could alter committed seed-file content on Windows).
- **Response:** **defer-with-rationale** (scope is the user's explicit call). Mitigation folded into
  T-4304: a seed-file-content assertion runs on both OSes; if Windows autocrlf perturbs content,
  assert on a normalized form or a substring rather than an exact byte match.

## Confidence
proceed-with-caveats → C-001/C-002/C-003/C-004/C-005/C-006 fixed in the revised build-plan.md/
test-plan.md; C-007 verified clean; C-008 deferred (user's scope choice) with a Windows-CI
mitigation folded into the tests.

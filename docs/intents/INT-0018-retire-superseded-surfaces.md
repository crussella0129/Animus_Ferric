# INT-0018 — Retire superseded surfaces

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0018
- **State:** active
- **Work evidence:** [Sprint 130 build plan](../sprints/s130/sprint-plans/build-plan.md); T-12906 in [tasks](../work/tasks.md#minimal-code-int-0017)
- **Completion evidence:** none
- **Code evidence:** none yet
- **Test evidence:** none yet
- **Documentation evidence:** [Sprint 129 build plan §E](../sprints/s129/sprint-plans/build-plan.md)

## Intent

Sprint 129 (INT-0017) took the code as far as it could go without changing
what users can do. The rest of the redundancy sits in standalone surfaces
that are superseded, abandoned, or only exist to read formats nobody writes
any more. On 2026-11-07 the owner approved retiring all of them (T-12906).
CLAUDE.md requires an owner-approved intent before any standalone surface is
retired; this is that intent, with one acceptance criterion per retirement.

## Acceptance criteria

Each criterion removes the code, its tests and its documentation, and
records the user-facing replacement. Everything else keeps its behaviour.

1. **Tailscale Serve publication (E-1).** `ferric server up --tailscale`,
   the Tailscale LocalAPI and Serve adapters, their halves of
   status/down/doctor, the `ferric-lifecycle-test` binary and its
   bin-identity gate are removed. `ferric tunnel` (INT-0015) is the remote
   access route. Its Linux-only gap is recorded in T-12707. A registration
   that still carries Tailscale ownership is reported as unverifiable, with
   guidance to turn the serve off by hand.
2. **Schema-1 runfiles and `ferric server adopt` (E-2).** Only schema-2
   registrations are accepted. Anything else is unverifiable, with guidance
   to stop the process and delete the record. The `adopt` subcommand is
   removed.
3. **The paired Legacy-vs-Evidence autonomy runner (E-3).** The
   `bench autonomy` options `--paired`, `--control-bin`, `--candidate-bin`,
   `--control-sha256` and Evidence-arm selection are removed, together with
   the strict managed-server binding that only that mode used. The
   single-binary autonomy matrix stays. Old result rows still parse.
4. **The Legacy `Tool::run` dispatch path (E-4).** Tools have one execution
   path, prepare/commit; the default policy dispatches through it. The
   `ControlCapability` mirror (R-TOOLS-7) goes with it.
5. **The backend-free `ferric-cli` build (E-5).** The OpenAI HTTP backend is
   always compiled. `--mock` remains the offline mode. The no-backend CI job
   and every `not(feature = "backend-openai")` arm are removed.
6. **`HarnessPolicy::EvidencePlanner` (E-6).** The variant and every
   refusal of it are removed; the value is now invalid input.
7. **Replay of traces from before sprint 113 (E-7).** These are dropped:
   - the commit barrier for traces without `ActionsProposed`;
   - the legacy pause path;
   - the omitted-`controller_blocks` reconciliation;
   - the `done` alias.

   The sprint-129 nudge reconciliation (D-8) is kept.
8. **The tailnet research retriever (E-8).** `TailnetFsRetriever` and its
   SSH transport were never constructed. They are removed, and the backlog
   item that named them is closed.
9. **`tools/install.sh` and `tools/install.ps1` (E-9).** These developer
   wrappers are removed. The root installers and one documented
   `cargo install` remain.

## Rationale

These surfaces cost about 18K lines, by the sprint 129 analysts' estimates.
That is more than the whole behaviour-preserving refactor removed. Each one
is superseded (Tailscale Serve by `ferric tunnel`, the tools wrappers by
the root installers), abandoned (the Evidence controller A/B, the planner
arm), or exists only to read formats nobody has written since August.
Keeping them makes every reviewer pay for code that no current user path
needs.

## Alternatives

- **Keep them in maintenance.** Each would still need fixes, CI time and
  review, for no user benefit.
- **Feature-gate them.** That keeps the code and adds a build matrix, which
  is the opposite of the goal.

## Consequences

- **Windows remote access:** Windows users have no supported remote-access
  route until `ferric tunnel` supports Windows (T-12707).
- **Old engines and traces:**
  - An engine started by a Ferric from before August must be stopped by
    hand.
  - Sessions interrupted before 2026-08-26 no longer resume.
  - Traces that recorded `evidence_planner` no longer parse.
- **Builds:** offline-only builds are gone; `--mock` covers offline use.

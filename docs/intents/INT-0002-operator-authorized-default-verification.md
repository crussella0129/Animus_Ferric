# INT-0002 — Operator-authorized default verification

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0002
- **State:** proposed
- **Work evidence:** [T-11401 backlog](../work/tasks.md#book-v2-carry-forward-from-sprint-113)
- **Completion evidence:** none
- **Code evidence:** none
- **Test evidence:** none
- **Documentation evidence:** [Sprint 113 gap audit](../sprints/s113/sprint-research/research-report.md)

## Intent

Make meaningful verification available to an ordinary Ferric project without
requiring the operator to remember `--checks-file` on every invocation. The
project may provide fixed, operator-authored argv checks; the model may select
only a declared name. Ferric must not discover and execute arbitrary package
scripts, shell strings, hooks, or model-authored commands implicitly.

## Acceptance criteria

1. Animus Launch can deterministically scaffold a documented, operator-owned
   project check profile, and the model cannot edit or redefine its commands.
2. Query and supported product surfaces load that profile under explicit
   precedence rules without a repeated CLI flag; an absent profile executes
   nothing and preserves current compatibility.
3. Checks use fixed argv, bounded time/output, exact workspace containment, and
   the existing requirement that completion evidence be newer than mutation.
4. Malformed, unsafe, ambiguous, or externally replaced profiles fail before
   model inference or process spawn.
5. Tests cover Windows/Linux path behavior, configuration precedence, model
   visibility, no-implicit-execution negative paths, and a frozen small-model
   evaluation before any default-on performance claim.

## Rationale

Both supplied analyses identified default verification as the highest-leverage
missing capability. Sprint 113 confirmed that `run_check` is useful only when
an operator supplies a checks file, while its Evidence experiment remained
0/3. A safe default therefore means making authorization persistent and easy,
not guessing which repository commands are safe.

## Alternatives

- Automatically run `package.json`, Cargo, Make, or CI commands: rejected
  because repository text is not execution authority.
- Keep `--checks-file` as the only route: compatible but too easy to omit and
  leaves ordinary projects without completion evidence.
- Give the model shell access: rejected; it widens the authority boundary
  instead of improving verification.

## Consequences

Launch and configuration gain another durable artifact and precedence rule.
Operators must review the fixed commands once, but subsequent runs become
verification-capable without silently granting new execution authority.

## Transition history

- 2026-08-26: created as `proposed` from the supplied analyses and Sprint 113 wider-field audit.

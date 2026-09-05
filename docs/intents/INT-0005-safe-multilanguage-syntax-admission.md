# INT-0005 — Safe multi-language syntax admission

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0005
- **State:** active
- **Work evidence:** [T-11405 backlog](../work/tasks.md#book-v2-carry-forward-from-sprint-113); [T-12001 Python 0.5 maintenance](../sprints/s120/sprint-plans/build-plan.md)
- **Completion evidence:** none
- **Code evidence:** none
- **Test evidence:** [Sprint 120 accepted prepared-host/configuration/Python Test increment](../sprints/s120/sprint-tests/test-report.md)
- **Documentation evidence:** [Sprint 113 gap audit](../sprints/s113/sprint-research/research-report.md)

## Intent

Extend pre-publication syntax admission beyond Python to supported Rust and
JavaScript/TypeScript source using bounded, in-process parsing of the exact
candidate bytes. Syntax checks must never execute a compiler, interpreter,
package hook, workspace import, or model-authored command implicitly.

## Acceptance criteria

1. Each supported extension has a version-pinned, bounded in-process parser
   over exact candidate bytes with deterministic normalized diagnostics.
2. Absent/valid-to-invalid transitions block atomically; compatible repair of
   already-invalid source remains possible under an explicit matrix.
3. Unsupported extensions and parser-limit failures are recorded as typed
   unchecked outcomes rather than guessed success.
4. Tests prove no process resolution, `PATH` use, imports, temp/cache artifacts,
   or partial publication, including adversarial workspace customization files.
5. Parser identity and admission outcome are traceable and Legacy compatibility
   remains warning-only where already promised.

## Rationale

Sprint 113 removed a real Python interpreter-execution hazard and replaced it
with RustPython, but every non-Python file is still explicitly unchecked.
Rust/JavaScript coverage is the next safe step; invoking `cargo`, `node`, or a
package script before authorization would recreate the boundary just removed.

## Alternatives

- Run language compilers or linters automatically: rejected because they may
  execute build scripts, plugins, imports, or repository configuration.
- Treat syntax as an operator check only: safe but permits obviously malformed
  candidate bytes to publish before the authorized test runs.
- Block every unsupported language: too restrictive and incompatible.

## Consequences

Parser dependencies and language-version drift require deliberate maintenance.
Syntax validity remains only an admission signal, never proof of semantic
correctness or a substitute for operator-authorized checks.

## Transition history

- 2026-09-05: moved from `planned` to `active` when Build began T-12001 under
  the locked owner-approved plan. Existing Python admission maintenance only.

- 2026-09-05: moved from `proposed` to `planned` after owner approval of Sprint 120 T-12001. This maintains existing Python admission only; Rust/JavaScript expansion remains T-11405.

- 2026-08-26: created as `proposed` after the Sprint 113 Python boundary repair.

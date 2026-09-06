# Preliminary Plan Critique — Sprint 121 (initial)

Independent `build_boundary_review` assessment of the initial scratch proposal.
This is not owner approval or canonical Plan acceptance. No implementation or
execution was performed. The primary agent's revisions must be re-reviewed.

## Concerns

### C-001: Protect the trace and sidecar together

- **Where:** proposal E02-D; `bench_cmd.rs` trace retention.
- **Quote:** "Sidecars SHALL use unique create-new publication, not overwrite prior evidence."
- **Failure mode:** hidden-dep
- **Why it matters:** Existing trace retention uses overwriting `fs::copy`. A collision could replace a previous trace before sidecar creation refuses, invalidating digest-bound evidence.
- **Suggested response:** fix-in-plan — require no-clobber retention of both artifacts and a named collision/failure test preserving both prior byte sequences without a successful observation.

### C-002: Make the live fixture's overall bound enforceable

- **Where:** E04-B live verification.
- **Quote:** "overall lifetime (at most 180 s, including bounded setup and teardown)"
- **Failure mode:** hidden-dep / plan-test-mismatch
- **Why it matters:** Prepared startup alone permits 180 seconds, with separate request and cleanup budgets. Copying that journey or checking elapsed time afterward cannot enforce this overall bound.
- **Suggested response:** fix-in-plan — fixture-local phase budgets, reserved cleanup time, enforceable cancellation for synchronous startup and a named deterministic stalled-phase cleanup test.

### C-003: Generated resume must preserve the cap's declared context

- **Where:** E01-C.
- **Quote:** "its existing shell-correct command SHALL repeat that cap exactly."
- **Failure mode:** hidden-dep
- **Why it matters:** A cap valid at declared context 32768 can be invalid at the generated resume's default 4096.
- **Suggested response:** fix-in-plan — repeat effective declared context alongside an explicit cap; revalidate against fresh policy and test large-context round-trip and changed-policy refusal. Do not inherit tier authority or silently clip.

## Confidence

block

The scope is otherwise coherent. Complete synthetic full-ladder evidence can
exercise the actual shared single/fleet publication gate, but must remain
distinct from actual CLI failure-preservation and model qualification results.
Fresh owner approval and the later canonical critic/lock remain required.

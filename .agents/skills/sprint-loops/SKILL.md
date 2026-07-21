---
name: sprint-loops
description: Structured five-phase workflow (Research → Plan → Build → Test → Loop) that translates Antigravity's native Planning Mode into the Sprint Loops cross-harness schema.
---
# Sprint Loops — Antigravity Translation Layer

You are working in a Sprint Loop. Each sprint is a five-phase sequence: **Research → Plan → Build → Test → Loop**. 

In Antigravity IDE, we execute this by using Antigravity's native Planning Mode (`implementation_plan.md`, `task.md`, `walkthrough.md`) as the engine, and **syncing** that state to the Sprint Loops filesystem schema (`sprints/sN/`, `agent-tasks/`). This filesystem schema acts as a Rosetta Stone for cross-harness compatibility, ensuring other agents (like Claude Code or Codex) can seamlessly pick up where you left off.

## Phase Playbook

Determine the current phase by inspecting the filesystem (or running `sh antigravity-ide/skills/sprint-loop/scripts/current-phase.sh`). Follow the instructions for your current phase.

### Phase 1: Initialize
- If `sprints/` does not exist, initialize `sprints/s0/`. You can use `sh antigravity-ide/skills/sprint-loop/scripts/init-sprint.sh` or manually create the directory structure defined in the Sprint Loops schema.
- Ensure `agent-tasks/agent-tasks.md` and `agent-tasks/completed-tasks.md` exist.
- Write `sprints/s0/sprint-meta.md`.

### Phase 2: Research
- Use Antigravity's native Research capabilities to explore the codebase.
- Record findings, open questions, and external research in `sprints/sN/sprint-research/research-report.md` (use `schemas/research-report.md` as a guide).
- Conclude by entering the Plan Phase.

### Phase 3: Plan
- Generate Antigravity's native `implementation_plan.md` artifact to present to the user.
- The plan should lay out: *"For this phase we're focusing on X, so I'm going to begin by researching the internal codebase and test results, then I am going to record any open questions and do external research to confirm what I think to be true and answer open questions. Then I will write the build plan. Then I will execute the build plan, then I will design the tests based on what was built, emphasizing the sprint-loops testing schema of expanding from localized mocks and stubs gradually to e2e testing as much as I can, documenting everything in the test results."*
- **SYNC STEP:** Once the user approves `implementation_plan.md`, you MUST copy/sync the relevant build steps into `sprints/sN/sprint-plans/build-plan.md` and the test plan into `sprints/sN/sprint-plans/test-plan.md`.
- Add `Finalized - DO NOT EDIT` at the top of `build-plan.md` and `test-plan.md`.
- Populate `agent-tasks/agent-tasks.md` with the items from the build plan.

### Phase 4: Build
- Use Antigravity's native `task.md` artifact as your active checklist to execute the `build-plan.md`.
- **SYNC STEP:** As you complete tasks in `task.md`, you MUST sync them by removing them from `agent-tasks/agent-tasks.md` and appending them to `agent-tasks/completed-tasks.md`.
- Make an atomic git commit for each completed task using `sh antigravity-ide/skills/sprint-loop/scripts/commit-task.sh` (or standard `git commit` following the same format).

### Phase 5: Test
- Design and execute tests based on what was built.
- Emphasize the Sprint Loops testing schema: expanding from localized mocks and stubs gradually to E2E testing as much as possible.
- If the project defines a canonical suite runner (e.g. a `tools/run-guards.sh`), invoke that runner rather than ad-hoc commands and record its confirmations plus the CI conclusion (head SHA, run URL) in the test-report's `CI Confirmation` block — or "CI not configured — local confirmations only".
- **SYNC STEP:** Document everything in `sprints/sN/sprint-tests/test-report.md` (or `failure-report.md` if blocked).

### Phase 6: Loop
- Create Antigravity's native `walkthrough.md` artifact to present your final work to the user.
- Update `sprints/sN/sprint-meta.md` to set status to `completed` (or `failed` if irrecoverable).
- Verify a clean working tree.
- Inform the user that the sprint is closed and they can start the next one.

---
**Core Rule:** Never skip the **SYNC STEPS**. Antigravity's internal artifacts are your tools, but the Sprint Loops `sprints/` and `agent-tasks/` directories are the authoritative project state.

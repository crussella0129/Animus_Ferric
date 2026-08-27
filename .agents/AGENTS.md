# Agent Rules

The following rules apply to all agent interactions within this workspace:

## Sprint Workflows

- Use the installed Sprint Loop adapter when the user explicitly starts or
  resumes a sprint.
- Treat the tracked Book-v2 records as authoritative: semantic intent lives in
  `docs/intents/`, active and completed work in `docs/work/`, and sprint
  provenance in `docs/sprints/`.
- Work directly on `dev` and open one `dev` → `main` PR at sprint close. The
  owner performs the merge. Keep sprint history out of the project README.

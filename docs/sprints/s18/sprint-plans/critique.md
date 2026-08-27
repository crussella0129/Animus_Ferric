# Plan Critique — Sprint 18

> Self-critique against `prompts/plan-critic.md`.

## Concerns

### C-001: `find_files` overlaps `search_files`
- **Failure mode:** redundant-tool
- **Response:** **distinct + both needed.** `search_files` greps *content*; `find_files` matches *names*. "Find the file called X" and "find files containing X" are different navigation needs a small model hits constantly. The descriptions make the split explicit so the model picks the right one.

### C-002: Growing Ring 1 could cost tool-call reliability
- **Failure mode:** reliability-regression
- **Response:** **measured, not assumed.** The E2E re-bench (`--calibrate-rings`) is exactly this check — both models must still calibrate to `--max-ring 1` solid with the fuller ring. The whole rings thesis is that the grammar stays reliable; this verifies the new tools don't break it. (10 tools is still within Small's `max_tools`, so no trimming muddies the result.)

### C-003: `copy_file` permission on the source path
- **Failure mode:** over-strict-guard
- **Response:** **accept, matches `move_path`.** A single `Write` permission guards both endpoints (the source gets a Write-check too). That's identical to `move_path` and only ever *over*-denies (e.g. copying *from* `.ferric` is blocked) — safe, never unsafe. Consistent with the established pattern.

### C-004: Recursive directory copy left out
- **Failure mode:** incomplete-feature
- **Response:** **deliberate + explicit.** `copy_file` is file-only and *errors* on a directory source (not a silent partial). Recursive dir copy is a larger, riskier operation better deferred; the error tells the model exactly why. Mirrors `delete_path`'s recursive gate.

## Confidence
`clean` — two small builtins on proven templates (`search_files`/`move_path`), a single-crate change, AI-verifiable via unit tests + the rings-gate count, with the ollama re-bench confirming reliability held.

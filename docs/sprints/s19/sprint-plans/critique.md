# Plan Critique — Sprint 19

> Self-critique against `prompts/plan-critic.md`.

## Concerns

### C-001: Is `multi_edit` really Ring 2 vs a fancier Ring-0 edit?
- **Failure mode:** ring-misassignment
- **Response:** **Ring 2 is right.** `edit_file` (Ring 0) is one change a 1B drives 100%. `multi_edit` asks the model to plan an *ordered set* of changes and emit a nested array — meaningfully harder output. The calibration sweep is the arbiter: if a 7B can't drive it, the gate caps it at Ring 1, which is the whole point of measured rings. Placing it at Ring 2 lets that measurement happen.

### C-002: The local fleet can't reach Ring 2 by tier
- **Failure mode:** untestable-live
- **Response:** **solved by `--params-b`.** Benching at `--params-b 20` lifts the toolbench to the Medium ceiling so the sweep includes Ring 2 — a genuinely useful operator flag (bench at any tier), not a test hack. It measures whether *this model* drives `multi_edit` regardless of nominal tier. No 13B+ download required.

### C-003: Partial application would corrupt a file
- **Failure mode:** non-atomic-write
- **Response:** **atomic by construction + tested.** All edits apply to an in-memory working string; a single `std::fs::write` happens only if *every* edit validated. The "missing old → file byte-identical" test pins that nothing is written on failure.

### C-004: Ambiguous `old_string` (multiple matches)
- **Failure mode:** wrong-occurrence
- **Response:** **first-occurrence, like `edit_file`.** `replacen(_,_,1)` replaces the first match — consistent with the established Ring-0 edit semantics. Sequential application lets the model disambiguate by editing in order. Documented in the tool description.

## Confidence
`clean` — one small builtin on the `edit_file` template (atomic loop), a one-line toolbench flag, AI-verifiable via unit tests + the rings-gate count; the `--params-b 20` sweep is an honest live measurement with both outcomes valid.

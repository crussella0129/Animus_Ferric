# Plan Critique — Sprint 27

> Self-critique against `prompts/plan-critic.md` (no subagent spawn — autonomous loop).

## Concerns

### C-001: A heuristic guard risks false positives on legitimately repetitive tasks
- **Failure mode:** over-eager-guard
- **Response (fix-in-plan, already reflected):** the threshold (`STOP_AT=5`, ~6 same-tool turns) sits comfortably above realistic same-tool runs yet well below every tier's `max_turns` (Nano 15 … Large 40). A `Warn`/nudge one turn before the `Stop` gives a capable-but-stuck model a course-correction; the name-set granularity means a turn that mixes in any other tool resets the streak; `max_turns` remains the ultimate backstop. The tradeoff is documented in ADR-037. Tuning is a one-line const change if data later argues for it.

### C-002: ADR-031 says nudging the 1B doesn't help — is this guard pointless?
- **Failure mode:** wrong-goal
- **Response (reject the framing):** the goal is **not** to make the 1B succeed. It is explicitly (1) bound wasted compute on any stuck model and (2) emit a precise `no_progress` diagnostic distinct from `max_turns`. Both are achieved regardless of whether the model can be coaxed to complete. Stated as the honest scope in the plan + ADR.

### C-003: Does the new stop reason break the bench classifier?
- **Failure mode:** silent-misclassification
- **Response (verified in research):** `verify.rs::completed()` passes only on `None|task_complete|final_text` terminators, so `no_progress` is treated as a non-completion automatically — **no bench change needed**, and a flailing run now reads `no_progress` instead of the ambiguous `max_turns`. Confirmed by reading the function.

### C-004: Adding an `Event` variant — blast radius?
- **Failure mode:** enum-exhaustiveness-break
- **Response:** `Event` is a serde-tagged enum; in-repo consumers (`verify.rs` parse_trace, trace tests) match specific variants with catch-alls, and unknown tags already fall to `ParsedEvent::Unknown`. Additive + backward-compatible. The compiler will surface any exhaustive match without a wildcard during build.

### C-005: Guard ordering vs the repetition guard
- **Failure mode:** interaction-bug
- **Response:** progress runs *after* repetition each turn. Identical-sig turns trip repetition at 2 strikes (before progress's streak fills), so existing behavior is preserved (regression test asserts this). The progress guard only catches the different-args case repetition lets through — they're complementary by construction.

## Confidence
`clean` — small, additive, well-scoped change mirroring an existing proven primitive
(`RepetitionGuard`), fully covered by unit + integration tests on the deterministic scripted
harness, with the one real risk (false positives) bounded by threshold + warn + backstop and
documented honestly.

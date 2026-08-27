# Sprint 39 Plan Critique — Responses

Reviewed by a foreground plan-critic agent against `research-report.md` and actual source
(`event.rs`, `reader.rs`, `sink.rs`, `run.rs`, `lib.rs`, `message.rs`, `query.rs`, `mcp.rs`).
Overall verdict: `proceed-with-caveats`, 12 concerns (9 fix-in-plan, 2 defer-with-rationale, 3
confirmed non-issues/reject). All applied to `build-plan.md`/`test-plan.md`.

### C-001 — "no `SessionEnd`" doesn't mean "N complete turns"; a killed process dies mid-turn (significant)
**Failure mode:** the plan's ONLY reconstruction fixture was "N complete turns, no SessionEnd" —
but the realistic shape of an interrupted process is a dangling `TurnStart` with no matching
`TurnEnd` (killed mid-provider-call), which the plan never specified how to handle.
**Response:** fix-in-plan. T-3903 now explicitly discards a dangling final turn (no `TurnEnd`) —
not committed to `messages`/`turns`, mirroring `run()`'s own behavior of never pushing anything for
a turn until its completion arrives. New test `replay_discards_a_dangling_mid_turn`.

### C-002 — `resumed_from`'s exact semantics (session id vs. path) were ambiguous; resume-chains undecided
**Failure mode:** the research report's own wording ("the prior session's id or trace path") was
ambiguous, and T-3903's `source_session: String` field name suggested an id without confirming it.
**Response:** fix-in-plan (cheap, clarifying). T-3901 now states explicitly: `resumed_from` stores
the original session's `session` id (already stamped on every trace line by `JsonlSink`), not a
path — stable even if files move. Resume-of-a-resume chains are explicitly allowed and need no
special handling (`replay()` only ever reads the one target file).

### C-003 — terminator-ordering placement was unpinned — the plan's own key correctness claim was unverified (the most significant finding)
**Failure mode:** T-3902 said "trace the terminator's ToolCall" without specifying WHERE in the
dispatch loop, which is exactly the ambiguity that could silently break trace-order for a
multi-tool-call turn with the terminator in the middle (e.g. `[tool_a, task_complete, tool_b]`).
**Response:** fix-in-plan. T-3902 now pins the trace-write to the exact position of the existing
`continue` inside the per-call loop (preserving `actions`' original order). New test
`replay_preserves_terminator_position_mid_turn`, with the terminator deliberately NOT last.

### C-004/C-006 — `nudged_for_no_action`/`truncated_once` are session-scoped one-shot flags, not per-turn/lookahead checks
**Failure mode:** T-3903's original wording ("not the final turn") described a lookahead-based
per-turn rule that doesn't match `run()`'s real state machine (a plain boolean, set once, never
reset) — risking either double-nudging or a broken rule requiring knowledge `replay()` can't have
while walking forward.
**Response:** fix-in-plan. T-3903 now states replay tracks the SAME one-shot boolean semantics
`run()` uses, and explains why this is sufficient: the `AlreadyStopped` gate (checked first, before
per-turn reconstruction) guarantees any surviving trace has at most one eligible turn for each flag
(a second occurrence would have produced `EmptyCompletion`/`TruncatedAction`, i.e. a `SessionEnd`,
already excluded).

### C-005 — the `TextXml` parse-error approximation was disclosed but never tested
**Failure mode:** the plan named the approximation (generic fallback text) as accepted, but no test
proved the fallback actually engages gracefully rather than panicking or producing something empty.
**Response:** fix-in-plan. New test `replay_reconstructs_xml_parse_error_nudge_falls_back_to_generic`
+ a new EARS clause on T-3903.

### C-007 — "shared formatter" (singular) risked collapsing 5-6 distinct nudge templates into one generic one
**Failure mode:** repetition-guard and no-progress-guard warnings have genuinely different wording;
a "one parameterized formatter for all nudges" implementation would functionally corrupt both.
**Response:** fix-in-plan. T-3903 now explicitly enumerates extracting each of the ~6 distinct
templates as its own small function. New test `replay_reconstructs_no_progress_guard_nudge`
alongside the existing repetition one, proving the two aren't collapsed.

### C-008 — confirmed non-issue: `result_message` visibility change
**Failure mode:** none — the critic verified `result_message` is a private free function in the
SAME crate (`ferric-loop`); making it `pub(crate)` for `replay.rs` is a trivial, safe visibility
change, exactly as the plan already described.
**Response:** reject (confirms the plan, no change needed) — included in the critique for
completeness since it was explicitly asked to verify.

### C-009 — resuming silently freezes the system message; `--prompts-dir`/`Animus.md` become no-ops with no signal
**Failure mode:** a user resuming after editing `Animus.md` would reasonably expect the edit to
apply — it silently won't, since `messages` is seeded from the replayed trace's own frozen system
message, not re-composed. Nothing in the original plan said so.
**Response:** fix-in-plan, matching this project's own established practice (ADR-048's
masking-hazard write-ups) of naming silent-no-op risks explicitly rather than leaving them implicit.
T-3904 documents the frozen-system-message behavior; T-3905 adds a stderr note when `--resume` is
combined with `--prompts-dir`/an `Animus.md`-bearing workspace, plus a new EARS clause and test
(`cli::resume_with_animus_md_prints_ignored_note`). `PolicySelected`/`PromptComposed` are clarified
to still be written fresh per-run (they record the actual continuation's tier/budgets, which may
legitimately differ from the original — harmless, unlike protocol, which IS validated).

### C-010 — no test round-trips a REAL `run()`-produced trace through `replay()`; every test hand-builds fixtures
**Failure mode:** every existing test in the plan constructs its trace fixture directly via
`JsonlSink::write_event` calls, never by actually running `run()` and feeding its real output back
into `replay()`. This leaves order-of-emission drift between what `run()` actually writes and what
`replay()` assumes structurally unguarded — the critic correctly identified this as the single
strongest missing regression test for exactly this reason.
**Response:** fix-in-plan. New test `real_run_then_replay_then_resume_reaches_task_complete`: a
genuine round-trip — real `run()` → truncate the real trace (drop `SessionEnd`) → real `replay()` →
a second real `run()` with `resume: Some(...)` → asserts `TaskComplete`. Added to T-3904's test
section.

### C-011 — "`--resume` + an extra prompt" is structurally the same mechanism as the rejected use-case 2, unacknowledged
**Failure mode:** the plan presented "resume + extra prompt" as unambiguously use-case-1
("continuing the same task"), but mechanically it's identical to the rejected "follow up on a
completed task" idea — the only real boundary is the `AlreadyStopped` gate (can't resume a finished
session), not the absence of the extra-prompt flag itself. Worth naming, not glossing over.
**Response:** defer-with-rationale, per the critic's own suggested resolution — no code change,
just one explicit acknowledging sentence added to T-3906's ADR-049 description, naming the tension
directly (matching ADR-047's precedent of explicit scope-bounding rather than silent overlap).

### C-012 — confirmed non-issue: `ferric mcp`'s `run_one` correctly always passes `Some`
**Failure mode:** none — the critic verified `McpServer::handle_tools_call` always requires a real
`"prompt"` MCP argument before calling `run_one`, so the mechanical `&str`→`Some(&str)` wrap at that
one call site is genuinely behavior-preserving, exactly as T-3904 claimed.
**Response:** reject (confirms the plan, no change needed) — included for completeness, the critic
was explicitly asked to verify this claim against `mcp.rs`.

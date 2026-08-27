# Sprint 89 — Research Report

## 1. Goal

Clear ADR-075's two remaining small items (E1, E4). Both are unambiguous fixes,
unlike E2 which is a posture decision for the user.

## 2. E1's fix had a trap in it

The obvious fix is "don't pass the approver to the sink gate when accept-edits is
on". That produces one prompt — and silently discards the sink's *reason* for
asking, which is the only place the taint is disclosed. Worse, naively passing
`None` would make `RequireApproval` fall through to denial, so the human's
approval would be ignored and the call blocked anyway.

So the fix merges the question instead: the accept-edits preview carries the
taint warning, and an approval there is honoured by the sink gate. One prompt
with strictly more information than either gate had alone.

`an_approved_tainted_write_actually_happens` exists specifically because a
prompt-count assertion alone would have passed the broken version.

## 3. E4 was a two-line judgement, not a design question

`run_chat` returns `ExitCode`, so `?` is unavailable — which is an argument for
*reporting*, not for discarding. Latching the warning to once per session keeps a
persistent cause (a full disk) visible without a per-event flood.

## 4. What is left

E2 is the only genuinely open question, and ADR-078's live measurement means it
is now evidenced from both sides: substring taint is fragile against paraphrase
in *both* directions. Re-tuning the constant is explicitly not a fix. That is the
user's call to make.

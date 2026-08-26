# Sprint 89 Meta

- **Sprint number:** 89
- **Start timestamp:** 2026-07-25T12:57:05Z
- **End timestamp:** 2026-07-25T13:40:00Z
- **Model:** claude-opus-5
- **Exit status:** success
- **Token count:** not observable
- **Summary:** Clear ADR-075's two remaining small items — E1 (the human was
  prompted twice for one call) and E4 (chat discarded trace-write failures).

## Outcome

524 → **529 tests, 0 failures**; clippy 0; fmt clean.

**E1** — accept-edits and the sink policy's `RequireApproval` cover exactly the
same calls (both fire only on `Write`/`Execute`; a tainted `Read` is always
allowed), so with both live the human answered twice about one call. Fixed by
**merging the question rather than suppressing the second ask**: the preview now
discloses the taint, and that approval carries through to the sink gate.

**E4** — all 6 discarded trace writes in `chat.rs` now report, latched to one
warning per session.

## The trap in E1's fix

The obvious fix — stop passing the approver to the sink gate — produces one
prompt and two bugs: the taint disclosure disappears, and `RequireApproval` with
no approver falls through to **denial**, so the human's approval would be ignored
and the call blocked anyway.

`an_approved_tainted_write_actually_happens` exists because a prompt-count
assertion alone would have passed that broken version.

## Still open

The **E2 posture decision** — the only genuinely ambiguous item, and now
evidenced from both sides (ADR-078): substring taint is fragile against
paraphrase in *both* directions, so re-tuning the threshold is not a fix. Plus
fleet re-calibration (nothing since sprints 25–26) and A5's sandbox (no Docker).

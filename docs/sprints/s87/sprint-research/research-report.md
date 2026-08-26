# Sprint 87 — Research Report

## 1. Goal

Fix sprint 86's F1, and use the live rig to validate what earlier sprints could
only check against mocks.

## 2. Why a fourth guard rather than tuning an existing one

The three guards are not mis-tuned; they are all *streak*-based, and a streak is
the wrong shape for a cycle. Alternation resets `repetition` (full signature) and
`progress` (tool names) on every single turn, and `failure` never engages because
an oscillating model's calls **succeed**. No threshold change to any of them
catches an A-B-A-B pattern — the question they ask is wrong, not the number.

So the new guard asks a different question: *over the last N turns, how many
distinct actions?* A sustained 2-cycle answers "two" however the turns interleave.

## 3. Threshold choices, made deliberately

- `MAX_DISTINCT = 2` — the unambiguous pathological case. A 3-cycle is
  deliberately not caught, with a named test saying so, because raising the bound
  starts catching legitimate short workflows that repeat with identical args.
- Window 8 / warn 6 — last in the ladder (repetition 2 < failure 3 <
  no-progress 5 < oscillation 8), so the sharper guards keep their cases and
  their diagnostics, and comfortably under Nano's 15-turn budget.

## 4. The tests that failed first were the useful ones

Two attempts at "genuine progress is never stopped" failed for reasons that were
*correct behaviour elsewhere*: reading nonexistent files tripped `FailureGuard`,
and eight same-name turns tripped `ProgressGuard`. Sharpening the test to
alternating names with fresh args isolated this guard — and produced the better
test, since that is exactly the shape a naive name-based window would kill.

## 5. G1 — found by trying to validate E2

`--research` produced no digests and no message. Discovered only because the
taint validation needed a real digest and there wasn't one. The E2 measurement
therefore stays synthetic, which is recorded rather than glossed.

## 6. The ZimaBoard2 model library

Online on the tailnet (100.95.64.15, SMB 445/139 open) but unreachable: no sshd
(port 22 refused), `net view` fails with RPC error 1702 over tailscale, no `Y:`
drive currently mapped, and ARK/Ark/models/Media/data/Public/CasaOS all failed as
share names. Needs the exact share name or a mapped drive from the user.

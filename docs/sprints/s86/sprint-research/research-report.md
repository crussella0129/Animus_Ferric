# Sprint 86 — Research Report

## 1. Goal

Run the live-model round ADR-075 called the highest-value next investment, and
verify the `--tailscale` path the user asked about.

## 2. Why this round mattered more than another static pass

503 mock-driven tests were green across three audit rounds. Both defects found
this sprint were **unreachable by that suite, by construction**:

- F1 needs a model that *chooses* badly. A scripted mock emits exactly what its
  author wrote; it never spontaneously oscillates.
- F2 needs the actual `tailscale` binary. No unit test had ever seen its output.

## 3. Findings

| # | Status | Summary |
|---|---|---|
| F1 | **PROVEN** | A-B-A-B oscillation defeats all three guards; 20 turns, 2 distinct calls, zero guard events. |
| F2 | **PROVEN, fixed** | `--tailscale` read `DNSName` from the JSON root; it lives at `Self.DNSName`, so the runfile silently kept the loopback URL. |

## 4. On not running `tailscale serve`

The verification target was "is the code using the command correctly". Executing
`tailscale serve --bg` would publish this machine's inference port to the tailnet
as standing configuration — an outward-facing change belonging to the user.
Read-only `status --json` and `serve --help` were enough to find the real bug and
to confirm the invocation shape was already right.

## 5. Honest limits

The round validated the *stack* but not several specific fixes: A1's cap went
unexercised because the model paginated, A2's taint needs `--research`, A5 needs
Docker. Those are recorded as open items rather than implied by the green run.

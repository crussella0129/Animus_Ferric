# Sprint 87 Meta

- **Sprint number:** 87
- **Start timestamp:** 2026-07-25T04:56:30Z
- **End timestamp:** 2026-07-25T06:20:00Z
- **Model:** claude-opus-5
- **Exit status:** success
- **Token count:** not observable
- **Summary:** Fix sprint 86's F1 (guard oscillation hole) with a fourth,
  windowed guard; use the live rig to validate F1's fix and A1's truncation cap.

## Outcome

507 → **518 tests, 0 failures**; clippy 0; fmt clean.

**F1 fixed and validated live on the exact scenario that found it:**
20 turns / zero guard events / `max_turns` → **8 turns / warned ×2 /
`oscillation`**. The three existing guards are streak-based, and a streak is the
wrong shape for a cycle — no threshold change to any of them would have worked.

**A1 validated live at last** (sprint 86 missed it because the model paginated).
Forced with a single 19,992-char line: the trace kept the full text, the model
got ~4,000 and reported it itself — *"which has been truncated for display."*

**G1 (new, live):** `--research` injected no research context and printed
nothing — a flag the user explicitly passed degraded into an ordinary run. Found
only because validating E2's taint finding needed a real digest and there wasn't
one. **So E2 stays unmeasured live**, and that is recorded rather than glossed.

## Blocked

A weaker second model for failure-mode testing. The ZimaBoard2 GGUF library is
online on the tailnet (100.95.64.15, SMB 445/139 open) but has no sshd, refuses
`net view` over tailscale (RPC 1702), exposes no guessable share name, and no
`Y:` drive is mapped. **Needs the exact share name or a mounted drive.**

Also still unrun: A5's sandbox (Docker absent) and fleet re-calibration.

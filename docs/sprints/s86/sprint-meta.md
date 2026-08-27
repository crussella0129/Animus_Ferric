# Sprint 86 Meta

- **Sprint number:** 86
- **Start timestamp:** 2026-07-25T04:25:47Z
- **End timestamp:** 2026-07-25T05:40:00Z
- **Model:** claude-opus-5
- **Exit status:** success
- **Token count:** not observable
- **Summary:** The live-model round — `ferric server` + llama.cpp +
  qwen2.5-coder-7b — plus verification of the `--tailscale` path.

## Outcome

503 → **507 tests, 0 failures**; clippy 0; fmt clean. First live-model run in
~60 sprints.

**The stack works.** `server doctor`/`up`/`status`/`down` all behave, and the
constrained agentic loop completed a real task — read_file → write_file →
task_complete, 3 turns, 36 s, correct output — with streaming and a full trace.

**Two defects the 503-test mock suite could not see:**

- **F1** — an A-B-A-B oscillation between two *successful* tools burned the
  entire 20-turn budget: 20 calls, **2 distinct `(name,args)` pairs**, **zero
  guard events**. All three guards key on consecutive-turn state and reset on
  alternation; `FailureGuard` never engages because nothing errors. Bounding
  wasted compute is the family's whole purpose.
- **F2 (fixed here)** — `--tailscale` read `DNSName` from the JSON root; it lives
  at `Self.DNSName`, so discovery always failed silently and the runfile kept the
  **loopback** URL while the command announced success.

Both were unreachable by mocks *by construction*: F1 needs a model that chooses
badly, F2 needs the real `tailscale` binary.

## Deliberately not done

`tailscale serve` was **not executed** — it publishes the machine's inference
port to the tailnet as standing configuration, which is the user's call. Read-only
`status --json` + `serve --help` were enough to find the bug and confirm the
`serve --bg <port>` half was already correct.

F1's fix (a windowed guard over the last N turns) is a design change to the guard
family — proposed in the report, entered in the backlog, not attempted here.

## Not validated live

A1's truncation cap (the model paginated `read_file`), A2's taint set
(`--research` unrun), A5's sandbox (no Docker), and the fleet calibration (still
sprints 25–26). All recorded as open rather than implied by the green run.

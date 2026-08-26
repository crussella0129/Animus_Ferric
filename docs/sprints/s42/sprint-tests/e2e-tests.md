# Sprint 42 E2E Tests

- **Status:** possible via `--mock` — the strongest end-to-end proof is the
  `chat_do_escalates_to_agentic_loop` + `chat_talk_turn_is_not_dispatched` subprocess pair
  (`integration-tests.md`), which drive a REAL `ferric chat --mock` binary with real stdin piping
  and real trace files. Together they demonstrate the full security boundary end-to-end: `/do`
  actually dispatches through the guarded agentic loop (a real workspace write + a full agentic
  trace), while a talk turn — even one whose text looks like a tool call — dispatches nothing.
  Filed under Integration rather than duplicated here (sprints 38–41 precedent).
- **Live-model smoke (manual):** a real conversational session against a live GGUF backend —
  `printf 'explain this repo\n/do add a hello.txt\n/exit\n' | ferric chat --backend openai
  --api-base http://127.0.0.1:8080/v1 --protocol grammar` — is a manual verification step, not
  automated, matching the project's no-live-backend-CI position (ADR-045). The `--mock` subprocess
  tests are the automated stand-in.
- **Deliberately deferred:** talk-mode streaming, a fancy TUI, and wiring `ferric chat` into the
  Animus IDE (a separate organ) — all named as ADR-052 deferrals, none needed to prove the
  hybrid-boundary mechanism this sprint.

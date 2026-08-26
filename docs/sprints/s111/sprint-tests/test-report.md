# Sprint 111 Test Report

## Outcome

**PASS for the live Monday server path.** No mock provider was used.

## Evidence

- `server doctor`: engine and model preflight passed.
- `server up`: launched `llama-server` PID 49512 on `127.0.0.1:8080` and wrote
  matching local/global registrations.
- Independent process/listener check: PID 49512 was the expected engine and
  owned the listener.
- HTTP: `/health` returned 200 with `status: ok`; `/v1/models` returned the
  loaded Qwen2.5-Coder-7B Q4_K_M model with context 8192.
- Real query: six constrained turns, six native tool calls, `task_complete`.
- Artifact: `math.py` defined a two-argument addition function and printed the
  result; the requested `temp` directory was absent.
- Trace: 50 records and six turns passed side-effect-free structural verify.
- Wall time: approximately 58 seconds for the E2E task.
- `server down`: exit 0; PID gone, listener gone, both runfiles gone.
- Postcondition: `server status` reported no registered server and exit 1.

## Caveats

The local llama.cpp distribution appeared CPU-only, so latency is the primary
demo risk. Ferric's built-in lifecycle probes are TCP-only and PID registration
needs hardening; independent checks covered those weaknesses for this run.

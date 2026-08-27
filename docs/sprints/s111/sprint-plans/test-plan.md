Finalized - DO NOT EDIT

# Sprint 111 Test Plan

1. `server doctor` passes for the exact `llama-server` binary and GGUF.
2. `server up` returns a runfile PID that exists, owns `127.0.0.1:8080`, and is
   the expected engine executable.
3. Local and global registrations agree on PID, port, and API base.
4. `/health` and `/v1/models` return HTTP 200; the model endpoint reports the
   expected Qwen model and 8192-token context.
5. A non-mock `tools/e2e_test.ps1` run creates valid `math.py`, deletes `temp`,
   ends in `task_complete`, and passes `trace verify`.
6. `server down` exits zero; the PID, listener, and both runfiles disappear;
   subsequent `server status` reports no registration with nonzero status.

# Held-Task Seal and Managed-Server Teardown

## Held evaluation

T-11312 is deliberately skipped because T-11311 had no qualifying candidate.
No H02, H03, H05, H06, or H07 evaluation episode ran; no held result row,
workspace, or trace was created. This preserves the causal rule that held
outcomes cannot guide candidate development.

One observer-boundary caveat is recorded rather than hidden: during Book
migration reconciliation, a read-only helper's contextual search surfaced one
H03 prompt line. The helper did not open the held trace or artifact, did not use
the line for a recommendation, and did not communicate its content to either
candidate-development agent. Thus no candidate change was informed by held
content and no held outcome was inspected, but H03's strict observer seal is
technically contaminated. H02 and H05–H07 remained wholly uninspected. Because
no candidate qualified, no held-task promotion claim is made.

## Process-cold lifecycle and teardown

Before screen 004, the prior managed server was independently attested as PID
33296, loopback listener owner 33296 on port 8080, context 8192, seed 42, one
slot, and the pinned model. `server down` removed PID 33296, the listener, both
local/global runfiles, and health reachability before the revision-2 server was
started.

The revision-2 server then started as PID 48468 with exact argv:

```text
llama-server -m C:\Users\<you>\Animus_Ferric\models\qwen2.5-coder-7b-instruct-q4_k_m.gguf -c 8192 -ngl 0 --seed 42 --parallel 1 --host 127.0.0.1 --port 8080
```

Independent pre-run checks matched PID 48468 to the loopback listener, both
runfiles, `/health`, and `/v1/models`; the model endpoint reported context 8192
and 7,615,616,512 parameters. After screen 004, `server down` reported PID
48468 stopped. Independent checks then found:

- no process with PID 48468;
- no listener on port 8080;
- no local `.ferric/server.json`;
- no global `%APPDATA%/ferric/server.json`; and
- an unreachable health endpoint.

The managed evaluation server is therefore fully down.

# Sprint 111 Research Report — Live Server Acceptance

## Objective

Answer the user's actual Monday question with direct evidence: launch Ferric's
server, use it for a real constrained task, and shut it down cleanly. An offline
mock path is not acceptance.

## Environment discovered

- The current backend-enabled release binary is under `target/release`; the
  installed Ferric on `PATH` is older and must not be used for this verdict.
- `llama-server` is installed and discoverable on `PATH`.
- A complete Qwen2.5-Coder-7B Q4_K_M model is available locally.
- A stale local Ferric runfile pointed at an absent process before the test;
  `server down` removed it and the target loopback port was confirmed free.
- The llama.cpp distribution does not expose CUDA/Vulkan runtime libraries and
  the server did not appear as an NVIDIA compute process. Expect CPU latency.

## Lifecycle audit

Ferric's engine argv construction, loopback binding, registration, discovery,
and ordinary teardown are coherent. The audit also found that built-in
readiness/status/doctor checks are TCP-only, `up` does not guard duplicate or
occupied-port launches, and `down` trusts a runfile PID. Independent PID,
listener-owner, HTTP, artifact, trace, and teardown checks are therefore part
of the live gate rather than relying on status output alone.

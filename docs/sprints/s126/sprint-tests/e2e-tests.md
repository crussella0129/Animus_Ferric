# Sprint 126 — end-to-end results: real Hermes through the Ferric valve

All runs use `cargo run -p ferric-valve --example hermes_pilot`. The runner
owns llama-server and each Hermes driver through `ferric_process::ProcessTree`
and records a reap proof for every child (`cleanup.jsonl`). The valve runs
in-process.

Hermes is the Animus Amalgam checkout at `f820dbf`, run in a disposable
`HERMES_HOME`. `USERPROFILE`, `APPDATA` and `LOCALAPPDATA` are redirected
into the session. The config mirrors Amalgam's lab: context pin, compression
off, environment probe off, reasoning echo on, title generation off, and
timeouts equal to the rate-derived deadline. Hermes gets the `file` toolset
only, so no model-authored shell runs.

The engine is llama.cpp **b10964 CUDA**, the build Amalgam's lab uses, with
server SHA-256 `aa2e1f5c…dec73`. Settings: 16,384 context, one slot, flash
attention, Q8 KV, no host prompt cache, `--jinja`, loopback.

Sanitized manifests, session ledgers, cleanup records and content-free
receipts are under [`e2e/`](e2e/). Local paths are replaced and receipts'
`model` is reduced to the file name.

## Operational repair found by running the real system

The first 7B bring-up ended `exit 1` in both sessions. Hermes refused
initialization with "context window of 16,384 tokens, which is below the
minimum 64,000 required", which is Hermes's 64K floor, the same one
Amalgam's Sprint 2 hit.

Hermes admits a smaller local window only when three things hold: a custom
local endpoint, compression disabled, and an explicit `model.context_length`
pin equal to the served window (`agent/agent_init.py::_enforce_minimum_context`).

The runner now writes Amalgam's lab session config (`evals/local_qualification/run.py::session_config`)
and the same environment isolation. The rerun passed. No valve code changed.

## `bringup_7b_valve` — Qwen2.5-Coder-7B Q4_K_M, all layers on GPU

Prefill ran at 2,641.6 tok/s and decode at 84.65 tok/s.

| Session | Result | Evidence |
|---|---|---|
| `lookup` | **complete** | `read_file {"path":"config.toml"}` → final answer "The port number configured in config.toml is 7342." Two constrained requests (`tool_calls`, then `stop`). The second request reused 2,531 cached tokens and evaluated 124. The thought round-tripped through `reasoning_content` echo. `message_hashes` extend: **true** |
| `no_tool` | **complete** | Answer "51" with zero tool calls, from one constrained request |

All children were reaped.

## `readiness_27b_valve` — Qwen3.8-27B UD-Q4_K_M, 24 of 66 layers on GPU

Prefill ran at 310.2 tok/s and decode at **3.39 tok/s**. The model SHA-256 is
`322e194f…23482`. The enforcement probe reported `enforced` against the real
server.

| Session | Result | Evidence |
|---|---|---|
| `lookup` | **complete**, 72.2 s | Three constrained actions: `search_files` → `read_file` → answer, with finish reasons `tool_calls`, `tool_calls`, `stop`. Decoded tokens per action were 37, 26 and 27. Continuations reused 2,561 and 2,656 cached tokens and evaluated only 70 and 97. `message_hashes` extend: **true**. Every tool call was valid (JSON-object arguments, a known tool) |
| `no_tool` | **complete**, 52.5 s | One constrained request; 50 decoded tokens; no tool call |

This meets INT-0012 AC-7: a real Hermes session completed tool calls and a
final answer through the valve on the pinned build and the reference model,
with receipts and a stable, extending prefix.

## `readiness_27b_cancel` — interrupt mid-generation (27B)

Hermes was asked for a 100-prime list. After 5 s of observed generation, the
runner wrote the interrupt file, and Hermes hard-cancelled, closing its
stream to the valve. The valve's receipt is **`cancelled`**, and llama-server's
`/slots` reported the slot **idle 4.88 s after the interrupt**, inside the
rate-derived bound. The driver exited 0, and every child was reaped.

## `pilot_27b` — native vs valve

The pilot results are recorded in [pilot/report.md](pilot/report.md) (T-12609).

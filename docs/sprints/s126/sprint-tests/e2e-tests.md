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

## `pilot_27b` — native vs valve (T-12609)

The full generated report is [pilot/report.md](pilot/report.md). The sanitized
manifest, session ledger, run list, exclusions and cleanup proofs sit beside
it.

**What ran.** Two runs used the same model, build, flags and corpus:

- **Pass 1** (`pilot-1790430708089`, Ferric `f06b57c`): all 24 sessions,
  covering 4 tasks × 2 arms × 3 repetitions, counterbalanced, in one
  llama-server process.
- **Pass 2** (`pilot-1790436617596`): Ferric `f06b57c` plus the working-tree
  `strip_verbatim` fix committed with T-12609. It re-ran `edit` and `create`,
  3 repetitions × 2 arms, after pass 1 revealed a runner defect.

**Runner defect found by operating the system.** `check_lab_root`
canonicalized the lab, which on Windows yields a verbatim path, and that path
became Hermes's working directory. Hermes then failed every file write in both
arms with `mkdir: cannot create directory '//?'`. `read_file` worked, so
`lookup` and `no_tool` were unaffected. `edit` and `create` completions
depended on the model guessing an absolute path. Native guessed faster, so
pass 1 misleadingly showed valve 7/12 against native 11/12.

Those 12 pass-1 write sessions are **excluded with their reason**
([exclusions.json](pilot/exclusions.json)) and remain in the ledger. They were
re-run on the fixed runner. The fix (`strip_verbatim`) has unit tests and a
live 7B check. Pass 2's clean runs make the contamination unambiguous: both
arms completed every write task.

**Result (24 included sessions):**

| | native | valve |
|---|---|---|
| Checked completions | **12/12** | **12/12** |
| All tool calls valid | 12/12 sessions | 12/12 sessions |
| Decoded tokens per request | 31.9 | 42.3 (+33%) |
| Decoded tokens per checked completion | 79.8 | 137.5 (+72%) |
| Wall time per session | 53.1 s | 69.7 s (+31%) |
| Prompt tokens served from cache | 58.1% | 67.4% |
| Rendered prefixes only grew | 12/12 | 12/12 |

**Reading.** On this corpus, the 27B's native template tool calling is already
reliable. Constraining it added **no measurable correctness**, a ceiling
effect, and cost about 30% more wall time. The cost is decode, not prefill.
The valve's prompt is smaller (it lists tools as `- name: description`, about
900 fewer prompt tokens), and its cache share is higher. It decodes more
because today's grammar forces a `thought` and a JSON wrapper around even a
one-word reply: `no_tool` decodes 3 tokens native against 51 through the
valve, and `lookup` 32 against 77.

That is INT-0013's question, answered for today's defaults: **with the
required `thought`, harness-owned decoding does not make this model faster,
and this corpus cannot show whether it makes it more accurate.** The next
measurements are the grammar-option arms INT-0013 already names (`F-none`,
`F-bounded`), `N-forced`, a Hermes-sized catalog, and harder tasks where
native calling fails.

**A second finding, about Hermes.** In pass 1's cap-exhausted valve sessions,
the final summary request re-rendered an early assistant turn, which broke
prefix extension. Hermes sends this request after hitting its tool-iteration
cap. The likely mechanism is that the request drops older `reasoning_content`,
which changes the valve's projected `thought`. It occurred only in sessions
exhausted by the write defect, and none of the clean sessions show it. It is
recorded for the valve's history-rendering work and as a lesson for Amalgam.

**Resource note.** The plan estimated about 1–1.5 GPU-hours. The actual total
was about 2.3 hours, including pass 2's roughly 22 minutes. Pass 2 repaired a
runner defect on the same model, host and build, a repeat attempt within the
approved resource class (Amalgam lesson L-18), not a new class.

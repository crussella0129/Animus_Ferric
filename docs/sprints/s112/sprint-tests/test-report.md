# Sprint 112 Test Report

## Pre-change baseline — commit `1b0c0df`

| Check | Result | Duration | Notes |
|---|---|---:|---|
| `cargo test --workspace --all-features` | PASS | 55.1 s | 0 failed; full workspace and doc tests |
| Retained Sprint 111 live server E2E | PASS | ~58 s | one Qwen2.5-Coder-7B task; six turns; not an accuracy estimate |
| Fresh frozen-binary Qwen live E2E | PASS | 69.395 s | exact 7.616B params, Small/params policy, six turns/calls, `task_complete`, artifact and trace verify pass |

The fresh run used `ferric-prechange-1b0c0df.exe`, SHA-256
`D9310E3CEA1BD32DD7D8F888150AAA752E42E55892AE746EB82E8AF1CE5103A8`,
against the Qwen2.5-Coder-7B GGUF, SHA-256
`509287F78CB4D4CF6B3843734733B914B2C158E43E22A7F4BF5E963800894D3C`.
The server-reported parameter count was 7,615,616,512; inference context was
8192. Independent checks matched the registered PID, process, loopback listener,
HTTP health, and `/v1/models` identity. No project or user config existed, and
the run pinned API URL, model, 7.616B params, context, grammar protocol,
temperature zero, no streaming, and an empty profile directory. The retained
trace has 50 records, six turns/calls, and SHA-256
`0E472AF7F1AFF5EEC6883B2DDBA2F70CD565752BA2F66FC95022F4F57D6D2FBD`.

An earlier Gemma preflight was excluded: it was launched correctly, but the
query was accidentally labelled 4B while the server reported 7.518B. It
generated long actions repeatedly for more than five minutes and was stopped;
it is not counted as a model failure or a baseline row.

One successful fixed task is still not an accuracy estimate. Repeated live
rows will be collected after the harness preserves trial identity and fails
closed.

## Build-phase checks

| Check | Result | Notes |
|---|---|---|
| Affected-crate tests, all targets/features | PASS | `ferric-core`, `ferric-tools`, `ferric-bench`, `ferric-cli` |
| Autonomy corpus audit | PASS | 24 tasks validate; every untouched seed fails its authoritative grader; no blocker after independent prompt/grader review |
| Autonomy focused tests | PASS | 17 bench + 8 CLI before the final full-suite run |
| Trace/recovery audit | PASS | Shared `TraceStructure` closes the reported sequence/proposal/checkpoint findings; recovery protocol tests green |
| Server launch hardening tests | PASS | registration/port/model preflight, HTTP status, child exit/readiness, PID+HTTP status/doctor |
| Release build (`backend-openai`) | PASS | final hardened binary SHA-256 `F6E636F80AD3AF22920C91A22AB0C5A1F0F4E8AFE56DFECEE77822061C8320F4` |

## Final quality gates

| Check | Result | Duration / notes |
|---|---|---|
| `cargo fmt --all -- --check` | PASS | final tree |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | PASS | 2.9 s |
| `cargo test --workspace --all-features` | PASS | 53.7 s; all workspace, integration, live sandbox/container, template-hygiene, and doc tests; only explicitly ignored fixtures/network tests skipped |
| `git diff --check` | PASS | no whitespace errors |

## Post-freeze live autonomy sample

All six included episodes used the real release `ferric query` process, grammar
protocol, the auto-discovered live `ferric server`, Qwen2.5-Coder-7B Q4_K_M,
8192 runtime context, temperature zero, isolated profiles, fixed executable
graders, retained traces, and exact model SHA-256
`509287F78CB4D4CF6B3843734733B914B2C158E43E22A7F4BF5E963800894D3C`.

| Task / variant | Contract | Objective | Duration | Evidence |
|---|---|---|---:|---|
| A05 / current | PASS | PASS | 48.408 s | four turns/calls, fresh completion gate, grader and trace verify pass |
| A01 / recovery | PASS | PASS | 79.079 s | `needs_input → task_complete`, one linked resume, correct clarification, both traces verify |
| R02 / recovery | PASS | PASS | 77.685 s | injected provider failure → linked resume → fresh gate; both traces verify |
| R08 / recovery | FAIL | FAIL | 161.464 s | two expected/observed resumes and workspace-mismatch refusal worked; final sequence ended `oscillation` without a passing objective/gate; all traces verify |
| H01 / recovery | FAIL | FAIL | 68.248 s | unnecessary clarification after three turns; authoritative grader failed |
| H01 / repository brief | FAIL | FAIL | 670.644 s | 26 turns, 26 calls, 11 tool errors, 15 mutations, one compaction, `max_turns`; grader failed |

The included sample is 3/6 contract and objective completion (50.0%, Wilson 95%
approximately 18.8%–81.2%) with zero infrastructure failures. Four expected
continuation operations across A01/R02/R08 were all observed and trace-linked,
but R08 still failed the coding objective. The one repository-brief pair was
failure/failure; it does not support enabling the brief by default.

This is a timed, unbalanced, single-trial sample—not the 72-coordinate 24×3
baseline, the 216-episode pass³ run, an external benchmark, or a general-agent
accuracy claim. A pre-freeze A05 run whose wording could reasonably be read as
a filename was corrected and excluded rather than scored against the model.

## Live-server acceptance

The final release hardened launch before the cold test: existing local/global
registrations, occupied/zero ports, zero llama context, and non-file model or
projector inputs fail before spawn. Ferric retains the child and writes runfiles
only after the engine-specific HTTP endpoint returns 200 while the child remains
alive. `status` and `doctor` now require PID liveness plus HTTP health.

Three independent process-cold lifecycles passed. Each began with no runfile,
listener, or matching model process; `doctor` passed; `up` returned zero; local
and global runfiles were byte-identical; the new PID resolved to the pinned
`llama-server` executable and exact model/context/loopback argv; exactly one
127.0.0.1:8080 listener belonged to that PID; `/health` returned `ok`;
`/v1/models` returned the exact model ID, `n_ctx=8192`, and
`n_params=7615616512`; and hardened `status` passed. PIDs were distinct across
cycles. Before every `down`, identity was rechecked. Every teardown removed the
PID, listener, model process, and both runfiles, and `status` returned exit 1.
The third cycle was intentionally left fully down.

Known boundary: `server down` still trusts the runfile PID without
executable/start-time identity. The controlled lifecycle was safe because those
were checked externally immediately before every kill; stale-PID binding is a
Sprint 113 task.

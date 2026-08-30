# Command Reference

Every `ferric` command and its key flags. Run `ferric <command> --help` for the
authoritative, always-current list. Global flag: `-v`/`-vv`/`-vvv` (verbosity, see
[Observability](#observability)).

Commands marked 🔵 need a running model (or `--mock`); the rest work offline.

---

## `ferric query` 🔵 — one agent run

```
ferric query [OPTIONS] [PROMPT]
```

Runs one workspace-scoped, constrained agent loop. `PROMPT` is required unless
`--resume` is given.

| Flag | Meaning |
|---|---|
| `--workspace <DIR>` | containment boundary (default: current dir) |
| `--trace-dir <DIR>` | query-only trace root; default `<workspace>/.ferric/trace`; relative paths resolve from the invocation directory |
| `--mock` | use the built-in scripted model — **no engine needed** |
| `--model <NAME>` | model id (required for a real backend) |
| `--api-base <URL>` | server URL (default: the running `ferric server`, else `http://localhost:1234/v1`) |
| `--api-key <KEY>` | API key, if your server needs one |
| `--params-b <N>` · `--quant <Q>` · `--family <F>` · `--ctx <N>` | model profile → run policy (defaults 1.2 / Q4_K_M / unknown / 4096) |
| `--temperature <T>` | sampling temperature (0.0 = deterministic) |
| `--protocol native\|grammar\|xml` | override the action protocol (default: from backend caps) |
| `--harness-policy legacy\|evidence\|evidence-planner` | autonomous control policy; fresh runs default to `legacy`, omitted resumes inherit the source trace |
| `--max-ring <N>` | cap the active tool ring (`0` = core only); restrict-only |
| `--profile-dir <DIR>` | read `model_profiles.json` for the earned tier/ring (default `benchmarks`) |
| `--checks-file <PATH>` | explicitly authorize fixed named verification commands and expose `run_check`; no checks are inferred when omitted |
| `--prompts-dir <DIR>` | prompt-element library for the system prompt |
| `--file <PATH>` | attach a file (repeatable); text folds into the prompt |
| `--modality image,audio,video` | declare media modalities for `--file` attachments |
| `--no-stream` | buffer output instead of streaming text + activity live |
| `--accept-edits` | pause before every mutating (`Write`/`Execute`) tool call and preview it; approve/reject from stdin (y/N) before it touches disk |
| `--resume <TRACE>` | continue an interrupted, incomplete session |
| `--research <QUERY>` | run the Ornstein research phase first (quarantined) |
| `--sink-action requireapproval\|deny\|warn` | CaMeL sink policy for tainted data (default requireapproval) |

An explicit `--trace-dir` must be disjoint from the canonical workspace in
both directions, and its existing components cannot be symbolic links or
Windows reparse points. Ferric validates it before and after directory
creation. A continuation of an externally stored trace must explicitly repeat
the same `--trace-dir` and `--workspace`; omission fails instead of falling
back to the workspace default. Every supported incomplete, resumable query
stop prints a `Resume:` command. Clarification alone adds
`--answer '<answer>'`. The command uses PowerShell syntax on Windows and POSIX-`sh`
syntax on Unix (`cmd.exe` is not supported). Successful terminal traces are
rejected by `--resume`, while an incomplete resumable `session_end` remains a
valid source. This is a low-level `query` option, not a high-level
run/status/resume/evidence workflow.

`evidence` is an opt-in experimental policy that binds supported mutations and
named checks to recorded workspace evidence. Its Sprint 113 frozen Qwen screen
remained 0/3 after both permitted revisions, so it is not presented as a
performance promotion. `evidence-planner` has no implementation and fails
before trace allocation or workspace mutation; it never falls back to
evidence-only execution. See the
[measured decision](sprints/s113/planner-decision.md).

---

## `ferric chat` 🔵 — hybrid REPL

```
ferric chat [OPTIONS]
```

An interactive REPL. At the `you>` prompt:

- plain text → **talk** (text only, no tools)
- `/do <request>` → **escalate** into the constrained agent loop
- `!<cmd>` or `/run <cmd>` → **passthrough**: run a shell command directly (no LLM),
  through the guarded `shell_exec` denylist
- `/help`, `/exit`, `/quit`

Flags mirror `query`'s model and run controls (`--workspace`, `--mock`,
backend/model, `--protocol`, `--max-ring`, …) plus `--no-stream` (streaming is
on by default in chat). The query-only `--trace-dir` is not a chat option.
Plain talk remains available under `evidence`, but `/do` refuses that policy
until chat can construct a truthful evidence continuation.

---

## `ferric server` 🔵 — manage the inference engine

```
ferric server up|status|adopt|doctor|down [OPTIONS]
```

Launches and supervises an OpenAI-compatible engine, pinned to `127.0.0.1`, and
writes a mirrored schema-v2 registration locally at `.ferric/server.json` and
in the user config directory so other commands can auto-discover it. `up`
rejects an existing registration or occupied port and registers only after the
spawned process owns the expected listener and returns HTTP 200 from its
engine-specific health endpoint.

`server up` options:

| Flag | Meaning |
|---|---|
| `--engine llama-server\|ollama` | engine (default `llama-server`) |
| `--model <PATH\|NAME>` | GGUF path (llama-server) or model name (Ollama) |
| `--mmproj <PATH>` | multimodal projector GGUF (image/audio/video) |
| `--ctx <N>` | context window (default 4096) |
| `--port <N>` | port on 127.0.0.1 (default 8080) |
| `--threads <N>` · `--gpu-layers <N>` · `--batch-size <N>` | edge/latency tuning (llama-server) |
| `--tailscale` | reserved; currently refused before spawn because durable Serve state cannot yet be rolled back safely |

- `status` — inventory local/global registrations, bind schema-v2 records to
  their process creation identity and listener owner, then report HTTP health
- `adopt --pid <pid>` — non-destructively verify one live schema-v1 process and
  conditionally upgrade its unchanged local/global registrations to schema v2;
  status/down print this complete command with the recorded numeric PID
- `doctor` — engine binary + launch inputs + registered PID/HTTP health
- `down` — stop only the uniquely verified process through its retained handle,
  prove that exact process exited and its registered listeners were released,
  then remove only unchanged registration bytes; stale-only records are cleaned
  without signalling a process when all of their listeners are proven absent

Lifecycle commands fail closed when a registration is malformed, unreadable,
conflicting, legacy schema v1 with a live PID, or otherwise unverifiable. HTTP
health alone never authorizes teardown. A wildcard/public listener makes
`status` fail; `down` refuses to signal, preserves every registration, and never
reports `stopped` because ownership is non-exclusive. Signal failure, exit
timeout/error, or a remaining, foreign, wildcard, or uninspectable listener
likewise preserves registrations and produces a non-success result.

Successful `down` reports `stopped`, `stale-cleaned`, or that the retained
process was already exited. Each registration is also reported independently
as `removed`, `already-absent`, `replacement-preserved`, `restore-failed`,
`removal-failed`, `held`, or another cleanup failure; any incomplete per-path
cleanup is an explicit partial, non-success result and includes every preserved
location (including same-parent holding paths).

Adoption never signals the legacy process. It verifies a retained process
handle, the closed engine executable, every available recorded argv coordinate,
and exact loopback listener ownership before conditionally replacing unchanged
aliases. The originating local schema-v1 registration must be present; a
global-only record is reported for repair instead of receiving an unusable
adoption command. Adoption rechecks the same generation afterward. On a failed
transition it rolls back only its own still-unchanged replacements and reports
any alias or preserved location it could not restore.

Native destructive lifecycle support is available on Windows and on
little-endian 64-bit x86_64/AArch64 Linux; other targets fail closed. See [The
Inference Server](server-configuration.md#registration-and-teardown-safety) for
recovery guidance.

---

## `ferric bench` 🔵 — the testbench

```
ferric bench ltd  [OPTIONS]     # single-turn tool fire rate + failure taxonomy
ferric bench full [OPTIONS]     # the L0–L6 capability ladder -> measured_level
ferric bench autonomy [OPTIONS] # internal repository-work/recovery baseline
```

`bench ltd` key flags: `--model`/`--models <a,b,c>` (fleet), `--protocol`,
`--iterations`, `--report <FILE>`, `--max-ring`, `--calibrate-rings`,
`--profile-dir`, `--params-b`.

`bench full` key flags: `--model`/`--models`, `--level <N>` (repeatable),
`--protocol` (default grammar), `--results-dir`, `--mock`.

`bench autonomy` uses the real server-backed path only and requires
`--model <ID>`. Key flags: repeatable `--task <ID>` and
`--variant <current|recovery|repository-brief>`, `--trials`, `--model-sha256`, `--ctx`,
`--server-state <cold|warm|unknown>`, `--results-dir`, and `--list`. Corpus v1
requires `--protocol grammar`; exit zero means the requested measurement is
complete and infrastructure-clean, not that the model passed every task.

See [testbench.md](testbench.md).

---

## `ferric icm` 🔵 — ICM agent delegation

```
ferric icm init <PATH>                 # scaffold a workspace (offline)
ferric icm plan <WORKSPACE> [--show-context]   # print the orchestration plan (offline)
ferric icm run  <WORKSPACE> [OPTIONS]  # execute the pipeline
```

`run` options: `--auto` (no review gates), `--from <N>`/`--to <N>` (stage range),
`--mock`, backend/model, `--params-b`, `--ctx`, `--max-ring`.
See [icm.md](icm.md).

---

## `ferric cron` — agentic cron

```
ferric cron add <NAME> --schedule <S> --command dream|query [--prompt <P>] [--mock]
ferric cron list
ferric cron run [--dry-run]
ferric cron watch [--interval <S>]
```

`--schedule` accepts an interval (`30s`/`15m`/`12h`/`2d`, `hourly`/`daily`/`weekly`)
**or a 5-field cron expression** (`0 2 * * *`, `0 9 * * 1-5`, evaluated in UTC).
All take `--workspace`. See [cron.md](cron.md).

---

## `ferric launch` — project scaffolder

```
ferric launch [--name <N>] [--path <DIR>] [--goal <G>] [--project-type <T>]
```

Deterministic, LLM-free. Scaffolds a git repo (main+dev) + a sprint-loop skeleton;
refuses to clobber a non-empty dir. Missing flags are asked interactively.

---

## `ferric mcp` 🔵 — MCP-stdio server

```
ferric mcp [OPTIONS]
```

Exposes exactly one MCP tool, `ferric_query`. Workspace/backend/model are
launch-time-fixed. Its model/run options parallel `query` and add `--resume`
and `--modality`; the query-only `--trace-dir` is not an MCP option.

---

## `ferric api` 🔵 — HTTP API

```
ferric api [--host 127.0.0.1] [--port 3581] [OPTIONS]
```

Bound to loopback by default. Requires the `backend-openai` build. Routes:
`GET /health`, `POST /v1/query` (one JSON response), and `POST /v1/query/stream`
(SSE stream of `StreamDelta`s). Body: `{"prompt": "..."}`. Other options mirror
`query`'s model/run controls; the query-only `--trace-dir` is not an API
option.

---

## `ferric dream` 🔵 — memory consolidation

```
ferric dream [--recent-traces <N>] [--memory-file <PATH>] [--model <NAME>] [--api-base <URL>]
```

Parses recent `.ferric/trace` files into a synthesized `MEMORY.md` (default
`.ferric/MEMORY.md`, 5 traces). Requires the `backend-openai` build + a model.

---

## `ferric revert` 🔵 — time travel

```
ferric revert <TRACE> <TURN>
```

Restores the workspace to turn `<TURN>`'s snapshot and truncates the trace so the
agent's memory resets there. Workspace root is automatically resolved from the trace's
`SessionStart` event (falling back to CWD). Needs a git workspace with per-turn snapshots.

---

## `ferric trace` — inspect traces (offline)

```
ferric trace cat <FILE>        # render a JSONL trace as a readable log
ferric trace verify <GOLDEN>   # validate transcript structure; executes no tools
```

---

## `ferric skills` — inspect installed skills (offline)

```
ferric skills list [--workspace <DIR>]   # list skills in .ferric/skills/ and how each authorizes
```

Skills are per-workspace instruction sets under `.ferric/skills/`. `list` shows
each one and whether it is standing-authorized (via `allowed_skills` in config)
or must be named per run with `ferric query --skill <name>` (ADR-091).

---

## Observability

`-v`/`-vv`/`-vvv` (info/debug/trace) raise the harness-internal log level, printed
to **stderr** (stdout stays a clean machine channel). `FERRIC_LOG` (or `RUST_LOG`)
overrides with a per-crate filter, e.g. `FERRIC_LOG=ferric_loop=debug`. Quiet
(WARN) by default. This is separate from the LLM JSONL trace.

---

## Configuration & data files

| Path | What |
|---|---|
| `.ferric/server.json` | local managed-server registration; schema v2 is mirrored in the user config directory for auto-discovery |
| `.ferric/trace/*.jsonl` | default per-session traces; `query --trace-dir` can place only query traces in a validated external directory |
| `.ferric/config.toml` | project config (backend, model, hooks, …) |
| `.ferric/cron/*.toml` | cron job definitions |
| `.ferric/MEMORY.md` | dream-mode consolidated memory |
| `Animus.md` | freeform system-prompt instructions (workspace root) |
| `.ferricignore` | additive path denylist (workspace root) |

See [Configuration](configuration.md).

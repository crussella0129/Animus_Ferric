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
| `--mock` | use the built-in scripted model — **no engine needed** |
| `--model <NAME>` | model id (required for a real backend) |
| `--api-base <URL>` | server URL (default: the running `ferric server`, else `http://localhost:1234/v1`) |
| `--api-key <KEY>` | API key, if your server needs one |
| `--params-b <N>` · `--quant <Q>` · `--family <F>` · `--ctx <N>` | model profile → run policy (defaults 1.2 / Q4_K_M / unknown / 4096) |
| `--temperature <T>` | sampling temperature (0.0 = deterministic) |
| `--protocol native\|grammar\|xml` | override the action protocol (default: from backend caps) |
| `--max-ring <N>` | cap the active tool ring (`0` = core only); restrict-only |
| `--profile-dir <DIR>` | read `model_profiles.json` for the earned tier/ring (default `benchmarks`) |
| `--prompts-dir <DIR>` | prompt-element library for the system prompt |
| `--file <PATH>` | attach a file (repeatable); text folds into the prompt |
| `--modality image,audio,video` | declare media modalities for `--file` attachments |
| `--stream` | stream text + thought live to stdout |
| `--accept-edits` | pause before every mutating (`Write`/`Execute`) tool call and preview it; approve/reject from stdin (y/N) before it touches disk |
| `--resume <TRACE>` | continue an interrupted, incomplete session |
| `--research <QUERY>` | run the Ornstein research phase first (quarantined) |
| `--sink-action requireapproval\|deny\|warn` | CaMeL sink policy for tainted data (default requireapproval) |

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

Flags mirror `query` (`--workspace`, `--mock`, backend/model, `--protocol`,
`--max-ring`, …) plus `--no-stream` (streaming is on by default in chat).

---

## `ferric server` 🔵 — manage the inference engine

```
ferric server up|status|doctor|down [OPTIONS]
```

Launches and supervises an OpenAI-compatible engine, pinned to `127.0.0.1`, and
writes `.ferric/server.json` so other commands auto-discover it.

`server up` options:

| Flag | Meaning |
|---|---|
| `--engine llama-server\|ollama` | engine (default `llama-server`) |
| `--model <PATH\|NAME>` | GGUF path (llama-server) or model name (Ollama) |
| `--mmproj <PATH>` | multimodal projector GGUF (image/audio/video) |
| `--ctx <N>` | context window (default 4096) |
| `--port <N>` | port on 127.0.0.1 (default 8080) |
| `--threads <N>` · `--gpu-layers <N>` · `--batch-size <N>` | edge/latency tuning (llama-server) |
| `--tailscale` | expose the port over Tailscale (needs the `tailscale` CLI) |

- `status` — health-check + base URL
- `doctor` — engine binary + model presence + reachability
- `down` — stop + remove the runfile

---

## `ferric bench` 🔵 — the testbench

```
ferric bench ltd  [OPTIONS]     # single-turn tool fire rate + failure taxonomy
ferric bench full [OPTIONS]     # the L0–L6 capability ladder -> measured_level
```

`bench ltd` key flags: `--model`/`--models <a,b,c>` (fleet), `--protocol`,
`--iterations`, `--report <FILE>`, `--max-ring`, `--calibrate-rings`,
`--profile-dir`, `--params-b`.

`bench full` key flags: `--model`/`--models`, `--level <N>` (repeatable),
`--protocol` (default grammar), `--results-dir`, `--mock`.

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
launch-time-fixed. Options mirror `query` plus `--resume` and `--modality`.

---

## `ferric api` 🔵 — HTTP API

```
ferric api [--host 127.0.0.1] [--port 3581] [OPTIONS]
```

Bound to loopback by default. Requires the `backend-openai` build. Routes:
`GET /health`, `POST /v1/query` (one JSON response), and `POST /v1/query/stream`
(SSE stream of `StreamDelta`s). Body: `{"prompt": "..."}`. Other options mirror
`query`.

---

## `ferric dream` 🔵 — memory consolidation

```
ferric dream [--recent-traces <N>] [--memory-file <PATH>] [--model <NAME>] [--api-base <URL>]
```

Parses recent `.ferric/traces` into a synthesized `MEMORY.md` (default
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
ferric trace verify <GOLDEN>   # replay with the mock to detect execution drift
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
| `.ferric/server.json` | the running server's runfile (auto-discovery) |
| `.ferric/trace/*.jsonl` | per-session traces |
| `.ferric/config.toml` | project config (backend, model, hooks, …) |
| `.ferric/cron/*.toml` | cron job definitions |
| `.ferric/MEMORY.md` | dream-mode consolidated memory |
| `Animus.md` | freeform system-prompt instructions (workspace root) |
| `.ferricignore` | additive path denylist (workspace root) |

See [Configuration](configuration.md).

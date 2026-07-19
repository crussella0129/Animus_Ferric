# Demo Guide

A hands-on, copy-paste walkthrough of **every** Ferric feature. Each section is
tagged:

- 🟢 **offline** — runs with `--mock` or no model at all. Great for demos.
- 🔵 **needs a model** — requires a running inference server (see
  [Getting Started §3](getting-started.md#3-external-run-a-real-model)).

Assumes `ferric` is on your `PATH` (built with `--features backend-openai`). Most
demos use a throwaway workspace:

```sh
mkdir /tmp/ferric-demo && cd /tmp/ferric-demo
```

> **The one external prerequisite for the 🔵 demos:** bring a model up first —
> `ferric server up --engine llama-server --model /path/to.gguf` — then the 🔵
> commands auto-discover it. See [Getting Started](getting-started.md).

---

## 1. One-shot agent query 🟢/🔵

The core loop: one workspace-scoped, policy-scaled, fully-traced agent run.

```sh
# 🟢 offline — the built-in scripted model:
ferric query --mock "create a project skeleton and finish"

# 🔵 with a model up:
ferric query "list the files here and summarize the code"
```

**What to look for:** the model plans, calls tools, and ends with `task_complete`.
A JSONL trace is written under `.ferric/trace/`. The workspace is a hard
containment boundary — the agent cannot touch anything outside it.

---

## 2. Read the trace 🟢

Every run is a replayable trace. Render the most recent one as a readable log
(`trace cat` takes a single file):

```sh
ferric trace cat "$(ls -t .ferric/trace/*.jsonl | head -1)"
```

**What to look for:** `session_start`, each turn's prompt/tool-calls/results, and
`session_end`. Tool *results* are stored full and untruncated — "if it isn't in
the trace, it didn't happen."

Verify a trace replays deterministically (regression check):

```sh
ferric trace verify .ferric/trace/<file>.jsonl
```

---

## 3. Observability — leveled diagnostics 🟢

Harness-internal logging, separate from the LLM trace. **Quiet by default**;
opened up with `-v`, and it goes to **stderr** so stdout stays clean.

```sh
ferric -vv query --mock "make a file" 2>diagnostics.log
cat diagnostics.log        # turn spans, tool dispatch timings, guard decisions
```

Target a single crate without a rebuild via an env filter:

```sh
FERRIC_LOG=ferric_loop=debug ferric query --mock "do a task" 2>&1 | head
```

`-v` = info, `-vv` = debug, `-vvv` = trace.

---

## 4. Tool rings — the earned grammar 🟢

Tools are organized into **rings** that widen as a model proves itself. Pin any
model to the Ring-0 core (the smallest, surest grammar):

```sh
ferric query --mock --max-ring 0 "do a task"
ferric trace cat "$(ls -t .ferric/trace/*.jsonl | head -1)" | grep "prompt assembled"
```

**What to look for:** the `prompt assembled` line lists the offered tools (`tools
[read_file, list_dir, …]`); with `--max-ring 0` only the navigate/mutate core is
offered. More background:
[testbench.md](testbench.md).

---

## 5. Chat REPL — talk, act, and passthrough 🟢/🔵

`ferric chat` is a hybrid REPL with three turn kinds:

```sh
ferric chat --mock
```

Then at the `you>` prompt:

| You type | What happens |
|---|---|
| `hello, what can you do?` | **talk** — text-only reply, no tools, no workspace changes |
| `/do write a hello.txt file` | **escalate** into the constrained agent loop (it can act) |
| `!ls -la` or `/run git status` | **passthrough** — runs the shell command directly, **no LLM**, through the guarded `shell_exec` denylist |
| `/help` / `/exit` | help / quit |

**Security demo:** try `!rm -rf /` — it's **blocked** by the command denylist even
in direct passthrough. Talk mode structurally has no action channel; only `/do`
(LLM-driven) and `!` (human-driven) can act.

---

## 6. Project scaffolder — `ferric launch` 🟢

Bootstrap a new git project (main+dev branches) + a sprint-loop skeleton,
deterministically and **LLM-free**:

```sh
ferric launch --name demo-proj --path /tmp/demo-proj \
  --goal "a CLI that formats JSON" --project-type Rust
ls -la /tmp/demo-proj        # README, .gitignore, agent-tasks/, decisions.md, git repo
git -C /tmp/demo-proj log --oneline --all
```

Run with no flags for an interactive interview. It **refuses to clobber** a
non-empty directory.

---

## 7. ICM agent delegation 🟢/🔵

Interpretable Context Methodology: the **filesystem is the orchestrator**.
Numbered stage folders run in order; each stage's `CONTEXT.md` scopes its context.

```sh
# Scaffold a 3-stage workspace (research -> script -> production):
ferric icm init /tmp/icm-demo

# Inspect the delegation plan — which files, at which layer, each stage gets
# (no model runs):
ferric icm plan /tmp/icm-demo

# 🟢 run the whole pipeline offline, straight through:
ferric icm run --auto --mock /tmp/icm-demo

# 🔵 with a model, with human review gates between stages:
ferric icm run /tmp/icm-demo         # pauses after each stage; Enter to continue, q to stop
```

**What to look for:** each stage runs contained to its own folder; `plan` shows the
five-layer context (identity / routing / contract / reference / working). Full
guide: [icm.md](icm.md).

---

## 8. Agentic cron — scheduled agent tasks 🟢/🔵

Schedule Ferric's own operations to run periodically.

```sh
cd /tmp/ferric-demo

# Add jobs (interval OR a cron expression):
ferric cron add nightly  --schedule "0 2 * * *" --command dream
ferric cron add quick    --schedule 6h --command query --prompt "summarize changes" --mock

ferric cron list                 # schedules, last-run, next-due
ferric cron run --dry-run        # which jobs are due (runs nothing)
ferric cron run                  # 🟢 runs due jobs once (the --mock query actually runs)
ferric cron run                  # again immediately -> "No jobs due" (state advanced)

# Watch loop (runs due jobs each tick until Ctrl-C):
ferric cron watch --interval 30s
```

**What to look for:** a `query`/`mock` job actually executes; state advances so it
won't re-fire until its interval elapses. A job's command is a **bounded enum**
(`dream`/`query`) — never an arbitrary shell string. Full guide: [cron.md](cron.md).

---

## 9. Time travel — `ferric revert` 🔵

Every turn snapshots the workspace to an orphaned git ref. Roll the workspace **and**
the trace back to any turn:

```sh
# After a real `ferric query` run in a git workspace:
ferric revert .ferric/trace/<file>.jsonl 2      # restore workspace + truncate trace to turn 2
```

**What to look for:** the workspace files return to their turn-2 state and the
trace is truncated so the agent's memory resets to that point. (Needs a real run in
a git repo; the mock loop also snapshots per turn.)

---

## 10. Session resume — continue an interrupted task 🟢

If a run is killed mid-task, replay its trace and continue the **same** task with
more turns (not a chat continuation):

```sh
# Resume an incomplete trace:
ferric query --resume .ferric/trace/<incomplete>.jsonl
```

A trace that already reached a stop reason is rejected. `ferric mcp --resume` does
the same over MCP.

---

## 11. `.ferricignore` — project denylist 🟢/🔵

Put paths off-limits to the agent, on top of the hardcoded guard:

```sh
cd /tmp/ferric-demo
printf 'secrets/\n*.pem\n' > .ferricignore
mkdir secrets && echo 'API_KEY=xyz' > secrets/prod.env
```

Now any tool call the agent makes against `secrets/` or a `*.pem` file is
**denied** at the registry chokepoint (rule `ferricignore`). It's **additive-only**
— it can only add denials, never relax the hardcoded floor — and the agent cannot
edit `.ferricignore` itself (it's write-protected).

**See the denial** (🔵, with a model that tries a blocked path) in the trace's
permission events, or trust the guard: the enforcement is unit- and
integration-tested. Details: [configuration.md](configuration.md#ferricignore).

---

## 12. Hooks — scripts on loop boundaries 🟢

Run your own scripts at `pre_turn` / `post_turn` / `on_error`. Configure in
`.ferric/config.toml`:

```sh
cd /tmp/ferric-demo
mkdir -p .ferric
cat > .ferric/config.toml <<'TOML'
[hooks]
post_turn = "echo turn-done >> hook.log"
TOML

ferric query --mock "do a task"
cat hook.log        # one line per turn
```

More: [configuration.md](configuration.md#hooks).

---

## 13. Persistent config + `Animus.md` 🟢

- **`.ferric/config.toml`** sets defaults (backend, model, params_b, ctx, …) so
  you don't repeat flags. Precedence: CLI flag > project config > user config >
  built-in default.
- **`Animus.md`** at the workspace root is freeform, user-authored instructions
  folded into the system prompt (like a `CLAUDE.md`).

```sh
cd /tmp/ferric-demo
echo 'Always write terse, well-commented code.' > Animus.md
ferric query --mock "do a task" 2>&1 | grep -i animus   # "Animus.md applied"
```

Details: [configuration.md](configuration.md).

---

## 14. Multimodal input 🔵

`ferric query --file` folds any text/code file into the prompt (any model); with
`--modality`, image/audio/video attach as content parts on a multimodal model:

```sh
ferric query --file notes.md "summarize these notes"                    # text, any model
ferric query --file photo.png --modality image "what's in this image?"  # needs a vision model + --mmproj
```

Full guide: [multimodal.md](multimodal.md).

---

## 15. Streaming output 🔵

See tokens (and the agent's live "thought") as they generate, instead of waiting
for the whole loop:

```sh
ferric query --stream "explain what this project does"
```

Under the constrained protocol, the final answer streams and a per-tool activity
line goes to stderr; the agent's scratchpad reasoning streams in dim gray.

---

## 16. The testbench — measure a model 🔵

Don't take a small model's tool-calling on faith — measure it.

```sh
# Single-turn fire rate + failure taxonomy + verdict band:
ferric bench ltd --backend openai --model <name> --protocol grammar --iterations 20 --report report.md

# The full L0–L6 capability ladder -> a measured_level:
ferric bench full --backend openai --model <name>

# Calibrate a fleet into one leaderboard (pick the smallest still-solid model):
ferric bench ltd --backend openai --models qwen2.5-coder:7b,llama3.1:8b,llama3.2:1b --protocol grammar --report fleet.md

# Calibrate the rings a model can reliably drive:
ferric bench ltd --backend openai --model <name> --calibrate-rings --profile-dir benchmarks
```

`bench full --mock` runs the ladder offline (against the scripted model) to
exercise the harness. Full guide: [testbench.md](testbench.md).

---

## 17. MCP server — expose Ferric to an IDE 🔵

`ferric mcp` is an MCP-stdio server exposing **exactly one** tool, `ferric_query`.
Point Claude Code / Cursor / an IDE at it. Quick handshake check by hand:

```sh
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | ferric mcp --mock
```

Workspace/backend/model are launch-time-fixed flags — a client can't redirect
them. Each `tools/call` runs the full constrained loop.

---

## 18. HTTP API — for web/mobile/IDE 🔵

Stream agent events over Server-Sent Events:

```sh
ferric api --port 3581 &          # binds 127.0.0.1 only
curl -N http://127.0.0.1:3581/v1/query/stream -d '{"prompt":"list the files"}'
```

Each SSE frame is a `StreamDelta` (thought / tool call / text). Built for the
`animus-ferric` VS Code extension.

---

## 19. Dream mode — consolidate memory 🔵

Offline knowledge extraction: parse recent traces into a persistent `MEMORY.md`:

```sh
# After several runs (so .ferric/traces has history), with a model up:
ferric dream --recent-traces 5 --memory-file .ferric/MEMORY.md
cat .ferric/MEMORY.md
```

---

## 20. Server lifecycle + edge/Tailscale 🔵

```sh
ferric server up --engine llama-server --model model.gguf --ctx 8192
ferric server status
ferric server doctor            # engine binary + model + reachability
ferric server up --tailscale    # also expose the port over Tailscale (needs the tailscale CLI)
ferric server down
```

Edge tuning knobs for Jetson/RPi: `--threads`, `--gpu-layers`, `--batch-size`.

---

## 21. Accept-edits — supervise every write 🟢/🔵

Pause before each mutating tool call, look at what the model wants to do, and
approve or reject it — without aborting the session.

```sh
ferric query --accept-edits --mock "create a file"
```

For every `Write`/`Execute` call the run stops and prints a preview to **stderr**:

```
── proposed: write_file ──
   target: out.txt
{
  "content": "hi",
  "path": "out.txt"
}
apply this edit? [y/N]
```

- `y` / `yes` → the call runs normally.
- anything else (`n`, empty line, EOF) → the call is **rejected**. Nothing touches
  disk, and the model is handed an `edit rejected by user` error result it can adapt
  to — the run keeps going, it does not abort.

Read-only calls (`read_file`, `list_dir`, …) are never gated, so you only get
prompted for things that actually change the workspace. The preview goes to stderr,
so `--stream` / piped stdout stays clean. *(Sprint 79, ADR-070.)*

---

## Quick reference: what needs a model?

| Offline (🟢) | Needs a model (🔵) |
|---|---|
| `query --mock`, `trace cat`/`verify`, `-v` diagnostics, `--max-ring` | `query` (real), `--stream`, multimodal |
| `chat --mock` (talk / `/do` / `!cmd`) | `chat` (real talk/escalate) |
| `launch`, `icm init`/`plan`/`run --mock`, `cron` (with mock jobs) | `icm run` (real), `dream`, `bench` |
| `.ferricignore`, hooks, config, `Animus.md` (mechanism) | `revert` (real run), `mcp`/`api` (real), `server` |
| `query --accept-edits --mock` (supervise writes) | `query --accept-edits` (real) |

See the [Command Reference](commands.md) for every flag.

# Your First Query

`ferric query` is the core of Animus: one workspace-scoped, fully-traced agent
run against a local model. Everything else — chat, MCP, benchmarking, the API —
is built on top of the same loop this command drives.

## The shape of a query

```sh
ferric query "<prompt>" [OPTIONS]
```

A query runs the model in a **constrained agentic loop**: the model proposes a
tool call, the harness validates and executes it inside the workspace, feeds the
result back, and repeats until the model calls `task_complete` or a budget/guard
stops it. By default, every step is written to a JSONL trace under the
workspace's `.ferric/trace/` directory.

## No model needed: `--mock`

You do not need a running model to see the loop work. The built-in mock runs a
scripted turn — a file write, then a structured completion — exercising the full
loop, guard, and trace path:

```sh
ferric query "do a mock task" --mock
```

This is the fastest way to confirm your build works and to see the trace format.

## A real run

For a real model, first stand up an inference server (covered in
[Installation & First Run](getting-started.md) and, in depth, in
[The Inference Server](server-configuration.md)):

```sh
ferric server up --engine llama-server --model /path/to/model.gguf
ferric query "list the Rust files and summarize lib.rs"
ferric server down
```

`query` **auto-discovers** the running server from `.ferric/server.json` — you do
not pass `--api-base` unless you are targeting a server you did not launch (for
example an already-running Ollama at `http://localhost:11434/v1`).

> Real-model queries require a binary built with `--features backend-openai`. A
> feature-less build still runs `--mock` and the offline tooling, and will tell
> you to rebuild if you ask it for a real run.

## The workspace is the boundary

A query operates on a **workspace** — by default the current directory, or an
explicit `--workspace <DIR>`. This is a hard containment boundary: the built-in
tools (`read_file`, `write_file`, `edit_file`, …) are checked against it through
`ferric-guard`, and a `.ferricignore` file can put further paths off-limits. The
model cannot read or write outside the workspace, and cannot read sensitive files
(`.env`, SSH keys, cloud credentials) even inside it.

## Keeping a query trace outside the workspace

`ferric query` alone accepts `--trace-dir <DIR>`. It is useful when the
workspace must remain free of Ferric trace state:

```sh
ferric query "do a mock task" --mock --workspace ./candidate --trace-dir ./evidence/traces
```

Omitting the flag keeps the compatible default,
`<workspace>/.ferric/trace`. A relative `--trace-dir` is resolved from the
directory where you invoked Ferric, not from `--workspace`.

An external trace directory must be fully disjoint from the canonical
workspace: it cannot equal, contain, or sit inside the workspace. Existing
path components must be ordinary directories, not symbolic links or Windows
reparse points. Ferric checks this before creating the directory and again
before allocating the trace. This query-level path check does not claim a
general high-level run/evidence workflow.

If the source trace is external, a continuation must repeat both its workspace
and the same external trace directory explicitly. Omitting `--trace-dir` is an
error; Ferric does not silently put the continuation under the workspace:

```powershell
# Windows PowerShell
ferric query --resume 'C:\evidence\traces\q-123.jsonl' --workspace 'C:\work\candidate' --trace-dir 'C:\evidence\traces'
```

```sh
# POSIX sh on Unix
ferric query --resume '/evidence/traces/q-123.jsonl' --workspace '/work/candidate' --trace-dir '/evidence/traces'
```

Whenever `query` reaches a supported incomplete, resumable stop, Ferric prints
a `Resume:` command with the exact workspace and external trace root needed to
continue. A clarification stop alone adds an `--answer '<answer>'` placeholder.
The printed syntax targets PowerShell on Windows and POSIX `sh` on Unix;
`cmd.exe` syntax is not promised.

A successful terminal trace is complete and is rejected by `--resume`. An
incomplete trace remains resumable even when its final `session_end` records
the incomplete stop (for example, exhaustion of the current turn budget); use
the printed command rather than guessing which paths to repeat.

## A few flags you will reach for early

| Flag | What it does |
|---|---|
| `--mock` | run the scripted mock; no model needed |
| `--workspace <DIR>` | set the containment boundary (default: current dir) |
| `--trace-dir <DIR>` | for `query` only, store traces in a safe external directory; relative to the invocation directory and repeated explicitly on external resume |
| `--file <PATH>` | attach a file (repeatable); text folds into the prompt, media attaches when `--modality` allows |
| `--stream` is on by default | text and tool activity print live; `--no-stream` to suppress |
| `--model <NAME>` | the model id your server expects |
| `--max-ring <N>` | cap the active [tool ring](advanced-tool-rings.md) (restrict-only) |

The full flag list is in the [Command Reference](commands.md); persistent
defaults (so you stop repeating flags) live in
[Configuration](configuration.md).

## Reading the result

When the run ends, Ferric prints the final answer and a one-line summary:

```
[task_complete after 3 turn(s); trace: .ferric/trace/q-1785504463287.jsonl]
```

That trace file is the source of truth for what happened — render it any time
with `ferric trace cat <file>`.

---

Next: [Chatting, and `/do`](basics-chat.md) — the interactive counterpart to the
one-shot query.

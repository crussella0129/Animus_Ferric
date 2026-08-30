<p align="center">
  <img src="docs/Animus.png" alt="Animus Ferric — agentic AI harness written in Rust" width="720">
</p>

# Animus Ferric

A local-first agentic coding harness written in Rust, purpose-built for **small local models (1B–14B GGUF)**.

**Licensing information for all Animus Project components held at https://github.com/crussella0129/Animus/blob/main/LICENSE**

Ferric is the Rust synthesis of the Animus lineage — [Animus](https://github.com/crussella0129/Animus) (Python), [Animus_Prion](https://github.com/crussella0129/Animus_Prion) (Go), and [fev](https://github.com/crussella0129/fev) (Go) — built on three convictions:

1. **The harness should own decoding.** Constrained generation (JSON-Schema / regex / CFG grammars) is driven end-to-end in the agent loop, so malformed tool calls are *impossible* rather than repairable.
2. **Behavior should scale to the model, deterministically.** A pure function maps a model profile (params, quant, context, measured capability level) to a run policy (protocol, turn/output budgets, tool count, and tool-ring ceiling). Small models get bounded runs and narrower grammars.
3. **The trajectory is the source of truth.** Every session writes a versioned JSONL trace — full conversation, tool calls, untruncated tool output, execution chain — replayable and diffable. If it isn't in the trace, it didn't happen.

Rust is not an implementation detail: the visible, demonstrable chain of ownership over state and execution is part of how you verify you control the agent.

## Documentation

Full docs live in **[`docs/`](docs/README.md)**. They are also assembled into a
book (via [mdBook](https://rust-lang.github.io/mdBook/), like *The Rust Book*) —
run `mdbook serve --open` from the repo root, or read the same pages on GitHub
below. The book's reading order is defined in [`docs/SUMMARY.md`](docs/SUMMARY.md).

- **[Getting Started](docs/getting-started.md)** — install, prerequisites, and the external steps (engine install, `ferric server`) before your first run. Includes a no-model `--mock` quickstart.
- **[Demo Guide](docs/demo-guide.md)** — a copy-paste walkthrough of every feature, each marked *offline* or *needs a model*.
- **[Command Reference](docs/commands.md)** · **[Configuration](docs/configuration.md)** — every command/flag; `.ferric/config.toml`, `Animus.md`, `.ferricignore`, hooks.
- Deep dives: [llama.cpp engine](docs/llama-cpp.md) · [testbench](docs/testbench.md) · [ICM delegation](docs/icm.md) · [agentic cron](docs/cron.md) · [Ornstein research](docs/ornstein.md) · [multimodal](docs/multimodal.md).

## Workspace

| Crate | Responsibility |
|---|---|
| `ferric-core` | Shared types + the deterministic scale function (ModelProfile → RunPolicy) |
| `ferric-trace` | Versioned TraceEvent schema, flush-per-event JSONL sink, tolerant reader |
| `ferric-provider` | Async `Provider` trait (constraint-carrying) + deterministic mock; real backends behind feature flags |
| `ferric-guard` | Hardcoded security: workspace boundary, permission checker, deny lists |
| `ferric-tools` | Tool trait, registry chokepoint, builtin file tools |
| `ferric-cli` | The `ferric` binary |

## Build

You need a recent stable Rust toolchain (`rustup`, edition 2024). Inference backends are **feature-gated** — pick the one(s) you want:

```sh
git clone https://github.com/crussella0129/Animus_Ferric.git
cd Animus_Ferric

# Recommended: the OpenAI-compatible HTTP valve — the constrained-decoding path.
# Talks to llama.cpp (llama-server), Ollama, or vLLM.
cargo build --release -p ferric-cli --features backend-openai
```

The binary lands at `target/release/ferric` (`ferric.exe` on Windows). A build with no backend feature still supports the offline/mock workflows; commands that need a real model tell you to rebuild with `backend-openai`.

**Put `ferric` on your `PATH`** (the examples below assume it is). The portable way — identical on Windows, Linux, and macOS — is `cargo install`, which builds in release *and* drops the binary into `~/.cargo/bin` (rustup already adds that directory to your `PATH`):

```sh
cargo install --path crates/ferric-cli --features backend-openai --force
# or use the wrapper, which does exactly this:
#   Linux/macOS:  ./tools/install.sh
#   Windows:      .\tools\install.ps1
```

> [!IMPORTANT]
> `cargo install` copies a **snapshot**. After you `git pull` or edit source, re-run it — a plain `cargo build` refreshes `target/release/` but **not** the copy on your `PATH`, so the `ferric` you invoke can silently lag the code (this is how a months-old binary kept offering a `--backend` flag that source had already removed). `--force` re-installs even though the version string (`0.1.0`) never changes.

## Using Ferric

| Command | What it does |
|---|---|
| `ferric server up\|status\|adopt\|doctor\|down` | Launch and identity-safely manage the local OpenAI-compatible inference server, bound to `127.0.0.1` only. |
| `ferric query "<prompt>"` | Run one workspace-scoped agent turn against a local model. |
| `ferric mcp` | Run an MCP-stdio server exposing one tool, `ferric_query`, to MCP clients (Claude Code, Cursor, an IDE). Workspace/backend/model are launch-time-fixed flags; each call runs the full constrained agent loop. *ADR-046.* |
| `ferric bench ltd` | Measure & diagnose tool-calling fire rate for a model (or a fleet — see below). |
| `ferric bench full` | Run the L0–L6 capability ladder and calibrate a model's `measured_level`. |
| `ferric bench autonomy` | Run the versioned 24-task internal repository-work baseline with retained traces, executable grading, and recovery evidence. |
| `ferric trace cat <file.jsonl>` | Render a session trace as a human-readable log. |

A typical loop — bring a server up, point Ferric at it, work, tear it down:

```sh
ferric server up --engine llama-server --model your.gguf
ferric server status                                        # prints base URL + health
ferric query "list the Rust files and summarize lib.rs"     # auto-discovers the server
ferric server down
```

> [!NOTE]
> `ferric server up --tailscale` is temporarily refused before side effects.
> Ferric will restore it only after it can compare-and-remove exactly the
> durable Tailscale Serve state it owns.

`query` and `bench` **auto-discover** the running server from `.ferric/server.json` (or your global `APPDATA` directory) — no `--api-base` needed. To target a server you didn't launch (e.g. an already-running Ollama), pass `--api-base http://localhost:11434/v1`. By default `query` runs the **constrained** path, which is the reliable one for small models.

`server up` fails closed when a local/global registration already exists, the
target port is occupied, or a llama.cpp model/projector is not a regular file.
It retains the exact spawned process object until the engine-specific health
endpoint returns HTTP 200, then publishes identity-bearing local/global
registrations without overwriting existing state. `status`, read-only backend
discovery, and `down` resolve those records against process-creation identity,
executable, argv, and exclusive loopback listener ownership; HTTP health alone
never authorizes teardown. A live historical schema-v1 record can be upgraded
without stopping it using the exact `ferric server adopt --pid <PID>` command
printed by `status`.

`ferric query` also takes workspace-local, guard-permitted files as input with `--file` (repeatable): text/code files fold into the prompt (works on any model), while image/audio/video attach as content parts when you declare `--modality` and the model can read them (Gemma 3n on the OpenAI valve). Attachments are bounded and pass through the same sensitive-path and `.ferricignore` checks as tool reads. See [docs/multimodal.md](docs/multimodal.md).

Streaming is on by default. Add `--no-stream` when a caller needs buffered output instead of live text and activity updates.

**Builtin model tools** (all workspace-scoped and security-checked through the guard). The always-on **core** (Ring 0) is `read_file`, `list_dir`, `write_file`, `make_dir`, `edit_file`, `delete_path`, `search_files`, `move_path`, and `copy_file`. **Ring 1** adds `find_files` and read-only `git_read`. **Ring 2** adds the structured mutation tools `multi_edit`, `apply_patch`, and `git_write`. Host `shell_exec` and `manage_task` controls are intentionally absent from every model grammar; the explicit `!cmd`/`/run` path in interactive chat is human-only. Tool vocabularies widen as a model proves it can call them reliably, and `ferric query --max-ring 0` pins any model to the smallest grammar (restrict-only; to widen, prove it with `ferric bench ltd` so `measured_level` promotes it).

Beyond the hardcoded guard, a project can drop a **`.ferricignore`** in the workspace root (gitignore-flavored — `secrets/`, `*.pem`, `data/private`) to put more paths off-limits to the agent. It is *additive-only*: it can only ever add denials on top of the hardcoded floor, never relax it (ADR-068), and the file is itself write-protected so the agent can't edit away its own restrictions.

## Test it with your own models

Ferric works with large and small models alike, but *how well* a small model drives the tools varies — so don't take it on faith, **measure it**. The testbench runs every tool many times, classifies *why* it misses, and grades the result, so you can dial a model down until quality drops.

**1. Bring a model.** Any of:

```sh
# llama.cpp — point the launcher at a GGUF on disk (needs `llama-server` on PATH):
ferric server up --engine llama-server --model /path/to/your-model.gguf [--mmproj mmproj.gguf] [--ctx 8192]
```

**2. Benchmark it** under the constrained path, and write a report:

```sh
ferric bench ltd --model <name> --protocol grammar --iterations 20 --report report.md
```

**3. Read the verdict.** `report.md` gives each tool a success rate, a **failure taxonomy** (`wrong_tool` / `malformed_args` / `no_action` / `parse_error`), and an acceptability band — **solid** (≥90%) / **marginal** (≥70%) / **unreliable** (<70%). Add `--protocol native` to compare the unconstrained path on the same model.

**4. Calibrate a whole fleet at once.** `--models <a,b,c>` benches each model and ranks them into one leaderboard, sorted best→worst — so you can pick the smallest model that's still *solid*:

```sh
ferric bench ltd --models qwen2.5-coder:7b,llama3.1:8b,llama3.2:1b --protocol grammar --report fleet.md
```

The generated leaderboard records success rates and verdicts for the exact
models and protocols you ran, so current results stay with their evidence
instead of becoming stale claims on this landing page.

**5. Calibrate the rings.** `--calibrate-rings` benches a model **ring by ring** and reports the highest ring it reliably drives — the recommended `--max-ring` to run it at (`ferric bench ltd … --calibrate-rings`). It's the demonstrated-reliability promotion: a model *earns* a wider grammar by proving it on the bench. Full walkthrough: [docs/testbench.md](docs/testbench.md).

## Portability

CPU-first. The baseline target includes Raspberry Pi / Orange Pi class aarch64 hardware; CI gates `cargo check --target aarch64-unknown-linux-gnu`. CUDA (NVIDIA, Jetson) and AMD paths are planned as specialized backends.

## Status

Active development. The single inference backend used by Ferric:

- **`backend-openai`** — an OpenAI-compatible HTTP valve (`llama.cpp` / vLLM) that enforces a harness-authored JSON-Schema constraint server-side. This is the constrained-decoding thesis working for small GGUF models — out-of-process, with pure Rust on Ferric's side. **It's the default and the reliable path.**

`--harness-policy evidence` is an opt-in experimental controller, not a
promotion claim. The default stays `legacy`, and the unimplemented
`evidence-planner` policy fails closed with no fallback. Its evaluation history
remains in the Sprint Loops Book records linked below.

The action protocol (`NativeTools` / `ConstrainedJson` / `TextXml`) is chosen
from the backend's real capabilities. Development follows the tracked
[Sprint Loops Book v2](docs/README.md). See [project intents](docs/intents/README.md),
[current work](docs/work/tasks.md), and
[completed work](docs/work/completed-tasks.md).

# Animus Ferric — Documentation

Ferric is a local-first agentic coding harness written in Rust, purpose-built for
small local models (1B–14B GGUF). This is the full documentation set.

## Start here

1. **[Getting Started](getting-started.md)** — prerequisites, build, and the
   external steps (installing an engine, bringing up a server) before your first
   run. Includes a no-model `--mock` quickstart.
2. **[Demo Guide](demo-guide.md)** — a copy-paste walkthrough of **every** feature,
   each marked *offline* or *needs a model*, so you can demo the whole product.
3. **[Command Reference](commands.md)** — every command and flag.
4. **[Configuration](configuration.md)** — `.ferric/config.toml`, `Animus.md`,
   `.ferricignore`, hooks, and environment variables.

## Feature deep-dives

| Doc | What it covers |
|---|---|
| [llama-cpp.md](llama-cpp.md) | Installing and driving `llama-server` (the recommended engine) |
| [testbench.md](testbench.md) | `ferric bench` — measuring & calibrating a model's tool-calling |
| [icm.md](icm.md) | ICM agent delegation — the filesystem as orchestrator |
| [cron.md](cron.md) | Agentic cron — scheduled periodic agent tasks |
| [ornstein.md](ornstein.md) | Ornstein — quarantined retrieval/research |
| [multimodal.md](multimodal.md) | `--file`/`--modality` image/audio/video input |
| [beast-zoo-spec.md](beast-zoo-spec.md) | Seed spec for the Animus "beast zoo" direction |

## The three convictions

Ferric is built on three ideas, each visible in the docs above:

1. **The harness owns decoding.** Constrained generation (JSON-Schema grammars) is
   driven end-to-end in the loop, so malformed tool calls are *impossible* rather
   than repairable. See [testbench.md](testbench.md).
2. **Behavior scales to the model, deterministically.** A pure function maps a
   model profile to a run policy (protocol, tool rings, turn budgets). Small models
   get small steps. See [Configuration](configuration.md) and [testbench.md](testbench.md).
3. **The trajectory is the source of truth.** Every session writes a versioned,
   replayable JSONL trace. See the trace demos in the [Demo Guide](demo-guide.md).

## Project records

The durable engineering record lives outside `docs/`:

- **`../decisions.md`** — the full ADR log (every architectural decision).
- **`../agent-tasks/`** — the task ledger.
- **`../README.md`** — the project overview + the sprint-by-sprint timeline.

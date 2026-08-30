# Getting Started

This guide takes you from a fresh clone to a working `ferric` binary and your
first agent run — including the **external steps** (installing an inference
engine, bringing up a server) you need before Ferric can drive a real model.

> **In a hurry / no model yet?** Every command has a `--mock` path that runs a
> built-in scripted model and needs **no external engine at all**. Jump to
> [Try it with no model (`--mock`)](#4-try-it-with-no-model---mock) and come back
> for the real-model setup when you're ready.

---

## 1. Prerequisites

| What | Why | Notes |
|---|---|---|
| **Rust toolchain** | build the `ferric` binary | `rustup`; the repo pins **Rust 1.96** (edition 2024) via `rust-toolchain.toml`, so `rustup` installs it automatically on first build. |
| **`llama-server`** (from llama.cpp) | the inference engine that runs a real model | Only needed for real-model runs, not `--mock`. See [step 3](#3-external-run-a-real-model). |
| **A GGUF model file** | the actual weights | e.g. a `qwen2.5-coder` or `gemma` GGUF. Not needed for `--mock`. |
| **Docker** (optional) | the containerized E2E harness + Ornstein web-research sandbox | Only for those specific features. |

Ferric is **local-first**: it only ever talks to `127.0.0.1`. Nothing leaves your
machine unless you explicitly point it at a remote server.

---

## 2. Build

Inference backends are **feature-gated**. Pick the OpenAI-compatible valve
(`backend-openai`) — it's the default, reliable, constrained-decoding path:

```sh
git clone https://github.com/crussella0129/Animus_Ferric.git
cd Animus_Ferric

cargo build --release -p ferric-cli --features backend-openai
```

The binary lands at `target/release/ferric` (`ferric.exe` on Windows) — call it
by path, or put it on your `PATH`. To install it on your `PATH` the same way on
Windows, Linux, and macOS, use `cargo install`, which builds in release and
copies the binary into `~/.cargo/bin` (rustup already adds that to your `PATH`):

```sh
cargo install --path crates/ferric-cli --features backend-openai --force
# wrappers that do exactly this:
#   Linux/macOS:  ./tools/install.sh
#   Windows:      .\tools\install.ps1
```

Re-run that **after every `git pull` or source change**: a plain `cargo build`
updates `target/release/` but *not* the copy on your `PATH`, so an installed
`ferric` can silently lag the code. `--force` re-installs despite the unchanged
`0.1.0` version. The examples below assume `ferric` is on `PATH`.

> **Built with no backend feature** (`cargo build -p ferric-cli`), only the
> offline tooling works — `trace`, `launch`, `icm init`/`plan`, `cron add`/`list`,
> and every `--mock` path. `query`/`bench`/`dream` against a real model, and the
> `api` command, need `--features backend-openai`.

Confirm it runs:

```sh
ferric --version        # -> ferric 0.1.0
ferric --help           # lists every command
```

---

## 3. External: run a real model

This is the part that happens **outside Ferric**: you install an inference engine
and hand Ferric a model. Ferric manages the engine's lifecycle for you, but the
engine binary and the model weights are yours to provide.

### 3a. Get `llama-server`

Download a prebuilt release (no compiling) from
<https://github.com/ggml-org/llama.cpp/releases> and pick the asset for your
hardware (CPU `-x64`/`-arm64`, or CUDA/Vulkan/etc.). Unzip it and make sure
`llama-server` is on your `PATH`. Full guide: [llama-cpp.md](llama-cpp.md).

### 3b. Get a GGUF model

Any `.gguf` file works. If you already use Ollama, its pulled models are plain
GGUF blobs you can reuse without re-downloading (see
[llama-cpp.md](llama-cpp.md#2-point-it-at-a-model--reuse-an-ollama-blob-no-re-download)).
A good starting model for a laptop is a 7B coder such as `qwen2.5-coder`.

### 3c. Bring the server up

`ferric server` launches and supervises the engine, pinned to loopback:

```sh
# Launch llama-server with your model, bound to 127.0.0.1:8080:
ferric server up --engine llama-server --model /path/to/your-model.gguf --ctx 8192

# (multimodal? add --mmproj mmproj.gguf. Edge tuning? --threads / --gpu-layers.)

ferric server status      # requires exact process/listener identity + HTTP health
ferric server doctor      # checks the engine binary, model file, and health
```

`server up` writes a schema-v2 `.ferric/server.json` in the current directory,
binding the registration to that process creation instance, executable,
arguments, and listener owner. Other commands **auto-discover** the running
server — you don't pass `--api-base` anywhere. When you're done:

```sh
ferric server down        # stops only the verified instance, then cleans its records
```

> **It also registers globally.** Alongside the local runfile, `server up`
> writes one to your user config directory (`%APPDATA%\ferric\server.json`, or
> the XDG equivalent), which is what lets a `ferric query` in *any* directory
> find the server. Useful, and worth knowing before you experiment: a
> throwaway `server up` in a scratch folder becomes the server every workspace
> discovers until you take it down. The global schema-v2 record names that
> originating local mirror, so `server down` from another workspace can clean
> the correct pair after it verifies the exact process.

`--model` is required for the managed `llama-server` path and must name a
regular file. `server up` also refuses an existing registration or occupied
port, and returns only after the spawned process is still alive and the
engine-specific HTTP health endpoint returns 200.

Lifecycle resolution inventories both local and global registrations instead
of silently preferring one. Malformed, conflicting, legacy-live, or
uninspectable state fails closed; HTTP health alone never authorizes teardown.
Stale-only records can be cleaned by `server down` without signalling a
process. See [Registration and teardown
safety](server-configuration.md#registration-and-teardown-safety) for legacy
recovery. `server up --tailscale` is temporarily refused before side effects;
existing `tailscale: true` records require targeted manual cleanup, never a
node-wide Serve reset.

If `status` finds a live historical schema-v1 record, it prints the explicit
non-destructive recovery command `ferric server adopt --pid <PID>`. Run that
from the workspace containing the local registration; Ferric upgrades only
unchanged records after verifying the closed engine executable, recorded
arguments, exact process generation, and exclusive IPv4-loopback listener.

> **Already running your own server?** (an existing Ollama, LM Studio, vLLM, or a
> remote llama-server) — skip `ferric server` entirely and pass
> `--api-base http://localhost:11434/v1` (and `--model <name>`) to `ferric query`.

### 3d. Your first real query

With the server up (from the same directory, so `.ferric/server.json` is found):

```sh
ferric query "list the Rust files here and summarize what the workspace does"
```

Ferric runs a full **constrained** agent loop: it plans, calls workspace-scoped
tools (`read_file`, `list_dir`, …), and finishes with a summary. Everything it did
is written to a replayable trace under `.ferric/trace/`.

---

## 4. Try it with no model (`--mock`)

To see the harness work **without any engine or model**, add `--mock` — a built-in
scripted provider stands in for the LLM:

```sh
mkdir /tmp/ferric-demo && cd /tmp/ferric-demo
ferric query --mock "create a file and finish"
```

You'll see the loop run to `task_complete`, a `ferric-mock.txt` appear, and a trace
written to `.ferric/trace/`. Render that trace as a human-readable log
(`trace cat` takes one file):

```sh
ferric trace cat "$(ls -t .ferric/trace/*.jsonl | head -1)"
```

`--mock` is how most features can be **demoed offline** — see the
[Demo Guide](demo-guide.md), which marks every step as *offline (mock)* or
*needs a model*.

---

## 5. Where to go next

- **[Demo Guide](demo-guide.md)** — a copy-paste walkthrough of *every* feature,
  offline where possible.
- **[Command Reference](commands.md)** — every command and flag.
- **[Configuration](configuration.md)** — `.ferric/config.toml`, `Animus.md`,
  `.ferricignore`, hooks, and environment variables.
- **Deep dives:** [testbench](testbench.md) · [ICM delegation](icm.md) ·
  [Agentic cron](cron.md) · [Ornstein research](ornstein.md) ·
  [multimodal](multimodal.md) · [llama.cpp engine](llama-cpp.md).

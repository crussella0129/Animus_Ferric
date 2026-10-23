<p align="center">
  <img src="docs/Animus.png" alt="Animus Ferric — the Iron of Animus Amalgam" width="720">
</p>

# Animus Ferric

**Ferric is the Iron of [Animus Amalgam](https://github.com/crussella0129/Animus_Amalgam).**
Amalgam is the Animus Project's fork of Hermes Agent, the Mercury. It owns the
conversation, tools, memory, human surface and authorization. What Hermes lacks
entirely is harness-owned constrained decoding: the harness authors the action
grammar and the backend enforces it, so a malformed tool call cannot be
produced. Ferric supplies that part.

- **`ferric-iron`** is the constrained-decoding core. It covers action-grammar
  authoring from any tool list (including Hermes's OpenAI-format tools),
  action parsing, capability-honest protocol selection, and the protocol's
  prompt conventions. It depends only on serde and thiserror. It is not a
  decoder: the backend (llama.cpp) enforces the token mask.
- **`ferric-valve`** is an OpenAI-compatible service that Hermes's custom
  endpoint points at. It turns Hermes's native-tools request into one
  constrained action for llama.cpp, then turns the action back into ordinary
  `tool_calls` or a reply, so Hermes keeps its loop, tools and history
  unchanged. See [crates/ferric-valve](crates/ferric-valve/README.md).

| Ferric owns | Amalgam owns |
|---|---|
| The core, the valve, and component-level evidence about them | Hermes integration, session-level qualification, and whether each approach advances |

The design target is mid-size quantized GGUF models: roughly 20–35B at about
Q4, on a consumer GPU plus system RAM with partial offload. The reference model
is Qwen3.8-27B at Q4. Small models (1B–8B) remain the floor and the regression
fleet. Whether constrained decoding makes that class faster or more accurate
is being measured
([INT-0013](docs/intents/INT-0013-constrained-decoding-on-midsize-quantized-target.md)),
not assumed.

## Install and run

Two steps: install `ferric`, then run it in a project folder.

### 1. Install

Linux and macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/crussella0129/Animus_Ferric/main/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/crussella0129/Animus_Ferric/main/install.ps1 | iex
```

The installer downloads the prebuilt `ferric` for your OS and CPU from the
latest GitHub release, checks its SHA-256, and installs it:

- **Linux and macOS:** into `~/.local/bin`. If that folder is not on your
  PATH, the installer prints the line to add.
- **Windows:** into `%LOCALAPPDATA%\Programs\ferric`, which it adds to your
  user PATH. Open a new terminal if `ferric` is not found.

> **Before the first release is published**, the installer builds Ferric
> from source with `cargo` instead. Install Rust from <https://rustup.rs>
> first; the build takes a few minutes.

### 2. Run

```sh
cd /path/to/your/project
ferric
```

On a machine with no engine or model yet, the first run shows what it needs
before downloading anything:

```text
To run a model on this machine, Ferric needs:
  • the llama.cpp engine, release b11780 (llama-b11780-bin-ubuntu-x64.tar.gz), from github.com/ggml-org/llama.cpp
  • Qwen2.5-Coder 7B Instruct (Q4_K_M, 4.7 GB, Apache-2.0), from huggingface.co/Qwen/Qwen2.5-Coder-7B-Instruct-GGUF
    chosen for this machine (16 GiB RAM); others: ferric setup --model 0.5b | 1.5b | 7b | 14b
Each file is pinned and checked against its SHA-256 before use.
Engine → ~/.local/share/ferric/engine   Model → ~/.local/share/ferric/models
Download now? [Y/n]
```

Press Enter. Both files download with progress and are verified, then the
session opens: choose whether to ask questions or allow file work, and type.
`/quit` ends the session. Later runs open the session straight away. An
interrupted download resumes where it stopped, including after Ctrl-C.

### What gets downloaded, and where

- **The engine.** The pinned llama.cpp release, as a CPU build for your
  platform. It is verified against the SHA-256 that GitHub records for that
  release asset.
- **The model.** Qwen2.5-Coder Instruct at Q4_K_M, Apache-2.0, pinned to a
  Hugging Face revision and checked against its SHA-256. Machines under 12
  GiB of RAM get the 1.5B model (1.1 GB); others get the 7B (4.7 GB).
- **The location.** Both go in Ferric's data folder: `~/.local/share/ferric`,
  or `%LOCALAPPDATA%\ferric` on Windows (`FERRIC_DATA_DIR` overrides it).
  One download serves every project.
- **What you already have.** A `llama-server` on your PATH, and GGUF files
  in `./models` or `FERRIC_MODELS_DIR`, are used as they are, and nothing is
  downloaded.
- **Scripts and CI.** Without a terminal, Ferric never downloads. It prints
  what is missing and the command to fetch it.
- **CPU only, for now.** Engines run on the CPU; GPU offload is planned.
  For the quickest answers on a laptop, choose the 1.5B model.

To fetch ahead of time, or choose another size:

```sh
ferric setup --list          # this machine, the engine, and every model with its size
ferric setup --model 1.5b    # 0.5b, 1.5b, 7b or 14b
ferric setup --yes           # no prompt (for scripts)
```

### Update and uninstall

- **To update,** run the install line again. It replaces `ferric` with the
  latest release, verified the same way. Your engine and models stay.
- **To uninstall,** delete the `ferric` binary, then Ferric's data folder
  (above). On Linux and macOS, also delete `~/.local/state/ferric` (service
  tokens and the tunnel plan) and `~/.config/ferric` (saved settings). On
  Windows, the data folder also holds those, and settings are in
  `%APPDATA%\ferric`. Each project keeps its own session traces in
  `.ferric/`.

See [Getting started](docs/getting-started.md) for building from source, the
manual engine route, and the no-model `--mock` mode.

The standalone assistant is otherwise in maintenance: fixes are accepted, new
features are not
([INT-0010](docs/intents/INT-0010-ferric-is-the-iron-of-amalgam.md)). The
install and first-run flow is the exception, active under
[INT-0016](docs/intents/INT-0016-install-run-done.md).

## Everyday commands

| Command | What it does |
|---|---|
| `ferric run` | Open the same interactive session as `ferric` with no arguments. |
| `ferric setup` | Download the engine and a model that fit this machine (one prompt, verified). `--list` shows the choices; `--model 1.5b` picks one; `--yes` skips the prompt. |
| `ferric status` | Describe configuration and available local resources. |
| `ferric explain` | Describe intended setup and ownership without probes or changes. |
| `ferric advanced` | Find the existing expert commands. |

Without a terminal, no-argument launch prints a short welcome and exits without
starting anything.

Ask mode has no file tools. File work requires permission for the displayed
folder on each session, or an explicit `--allow-edits` flag:

```sh
ferric run "Explain Rust ownership"
ferric run --allow-edits "Add a unit test for the parser"
```

File work uses the Evidence controller with conservative, unmeasured tool
limits. It grants no shell, hooks, or delegation. Existing expert commands,
including `query`, `chat`, `server`, `bench`, and `trace`, remain available
under their original names and through `advanced`.

## Running the valve with Hermes

With a llama.cpp server already listening on port 8080:

```sh
cargo run -p ferric-valve -- --upstream http://127.0.0.1:8080 --receipts receipts.jsonl
```

Then point Hermes's custom provider at `http://127.0.0.1:8090/v1`, and set
its `api_key` to the contents of the token file the valve prints at startup
(`~/.local/state/ferric/gate/valve.token` by default).

- The valve refuses to start unless the upstream demonstrably enforces a JSON
  schema.
- It binds loopback only, and every request passes the network gate (see
  [Security](#security-localhost-is-not-a-trust-boundary)).
- `--upstream-key-file` presents llama.cpp's own `--api-key-file` key
  upstream. Hermes's credential is never forwarded.
- It writes one content-free receipt per request.
- `--record-only` forwards every request unchanged while writing the same
  receipts, for a native comparison arm.

## Security: localhost is not a trust boundary

Every process on a machine, and every web page in its browser, can reach
`127.0.0.1`. In 2026 that turned local agent servers into remote code
execution (the opencode advisories). Ferric's HTTP services, `ferric-valve`
and `ferric api`, serve every request through `ferric-gate`. It runs four
checks in a fixed order, none of which can be configured away:

1. The `Host` header must name this listener, which defeats DNS rebinding.
2. Browsers are refused: any `Origin` or `Sec-Fetch-*` header is rejected.
3. The service's 256-bit bearer token is required. It is stored mode 0600
   outside every workspace.
4. Request bodies must be JSON.

No CORS header is ever sent, and nothing binds `0.0.0.0`.

```sh
ferric secure demo                       # offline: naive server 8/8 attacks land, gate 0/8
ferric secure probe http://127.0.0.1:3581 --token-file ~/.local/state/ferric/gate/api.token
```

For remote access without a coordination server, `ferric tunnel` writes a
self-hosted, two-peer WireGuard plan and brings it up on Mullvad's GotaTun.
The client can reach only Ferric's tunnel address, and the token is still
required. See [docs/security.md](docs/security.md) for the threat model, the
demo script and the stated limits.

## What preparation guarantees

A newly started engine stays owned by the session and is stopped and reaped on
exit. A borrowed server stays running. Ambiguous registrations are reported
without deleting them or stopping someone else's process.

Local launch defaults to CPU execution and a 4096-token context. These are
starting settings, not a hardware-fit or model-capability qualification.
No benchmark is required before conversation, and readiness is not evidence
of successful coding work. Status and explain do not claim completed workflow
checkpoints or perform health probes.

Cancellation is bounded during startup and provider requests. During file work,
an existing Git snapshot operation can delay cancellation until Git returns;
this limitation remains open. Session traces stay in `.ferric/trace`.

## Build from source and configure

The one-line installers above are the normal route. To build and install the
current source instead (Rust required; normal builds include the
OpenAI-compatible backend):

```sh
cargo install --path crates/ferric-cli --force
```

Reinstall after source changes; the installed copy is a snapshot. From a
checkout you can also run without installing: `cargo r` opens a session, and
arguments go after `cargo r --` (for example, `cargo r -- run --workspace
../my-project`). For a build without a real backend, use
`--no-default-features`; expert mock commands remain available.

[Configuration](docs/configuration.md) explains saved defaults and their
validation. [Command reference](docs/commands.md) covers the full expert
surface. [Server configuration](docs/server-configuration.md) covers manually
managed engines and Tailscale. [Testbench](docs/testbench.md) covers measured
capability. See the [documentation index](docs/README.md) for architecture and
contributing details.

Ferric belongs to the [Animus lineage](https://github.com/crussella0129/Animus).
[Licensing information for all Animus Project components](https://github.com/crussella0129/Animus/blob/main/LICENSE).

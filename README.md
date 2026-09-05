<p align="center">
  <img src="docs/Animus.png" alt="Animus Ferric — a local coding assistant written in Rust" width="720">
</p>

# Animus Ferric

A local coding assistant written in Rust, built for small GGUF models.

```sh
cargo r
```

From this repository, that opens a session in the current folder. Ferric asks
which model to use only when needed, then whether you want to ask questions or
allow file work. Type your question or task; `/quit` ends the session.

You need Rust and either an already configured model server, or an installed
`llama-server` plus an existing GGUF file in the workspace's `models` directory.
Ferric prepares the session and remembers the selected model. It does not
download engines or models. If resources are missing, it explains what to add.

## Everyday commands

| Command | What it does |
|---|---|
| `ferric run` | Open the same interactive session as `ferric` with no arguments. |
| `ferric status` | Describe configuration and available local resources. |
| `ferric explain` | Describe intended setup and ownership without probes or changes. |
| `ferric advanced` | Find the existing expert commands. |

For source execution, put arguments after `cargo r --`. For example,
`cargo r -- run --workspace ../my-project` selects another folder.
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

## Install and configure

Normal builds include the OpenAI-compatible backend. To install the current
source on your PATH:

```sh
cargo install --path crates/ferric-cli --force
```

Reinstall after source changes; the installed copy is a snapshot. For a build
without a real backend, use `--no-default-features`; expert mock commands remain
available.

[Configuration](docs/configuration.md) explains saved defaults and their
validation. [Command reference](docs/commands.md) covers the full expert
surface. [Server configuration](docs/server-configuration.md) covers manually
managed engines and Tailscale. [Testbench](docs/testbench.md) covers measured
capability. See the [documentation index](docs/README.md) for architecture and
contributing details.

Ferric belongs to the [Animus lineage](https://github.com/crussella0129/Animus).
[Licensing information for all Animus Project components](https://github.com/crussella0129/Animus/blob/main/LICENSE).

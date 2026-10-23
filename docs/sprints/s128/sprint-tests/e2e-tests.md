# Sprint 128 — End-to-end tests

## Fresh machine: install, setup, answer

The environment:

- **Home.** An empty `HOME`, with `PATH` limited to `$HOME/.local/bin:/usr/bin:/bin`
  (no `llama-server` and no `ferric`).
- **Installer.** `install.sh` ran against a local mirror of the release
  assets (`FERRIC_INSTALL_BASE_URL`), holding the `ferric` build under test
  packaged exactly as the release workflow packages it.
- **Engine.** It came from a local mirror of llama.cpp's release API
  (`FERRIC_ENGINE_API_BASE`). The mirror serves `b11780`'s Linux asset, a
  static `llama-server` built from llama.cpp source, with its SHA-256 digest
  in the release JSON, because github.com release downloads are blocked in
  this sandbox.
- **Model.** The real Qwen2.5-Coder 1.5B came from huggingface.co.

```
$ command -v llama-server ferric
(neither is installed)
$ curl -fsSL …/install.sh | sh
Downloading ferric-x86_64-unknown-linux-gnu.tar.gz ...
Verified SHA-256 af52b6d57f48725f77c0bff946c13c6bfa2f35e4ec71d4735f293767dc7833c3
Installed ferric 0.1.0 at $HOME/.local/bin/ferric
Next: cd into a project and run  ferric
The first run offers once to download the engine and a model that fit this machine.
$ ferric run "hello"   # non-interactive, nothing set up yet
Folder: $HOME/project
To run a model on this machine, Ferric needs:
  • the llama.cpp engine, release b11780 (llama-b11780-bin-ubuntu-x64.tar.gz), from github.com/ggml-org/llama.cpp
  • Qwen2.5-Coder 7B Instruct (Q4_K_M, 4.7 GB, Apache-2.0), from huggingface.co/Qwen/Qwen2.5-Coder-7B-Instruct-GGUF
    chosen for this machine (16 GiB RAM); others: ferric setup --model 0.5b | 1.5b | 7b | 14b
Each file is pinned and checked against its SHA-256 before use.
Engine → $HOME/.local/share/ferric/engine   Model → $HOME/.local/share/ferric/models
Run `ferric setup --yes` to download them, then run again.
(exit 1)
$ ferric setup --yes --model 1.5b
To run a model on this machine, Ferric needs:
  • the llama.cpp engine, release b11780 (llama-b11780-bin-ubuntu-x64.tar.gz), from github.com/ggml-org/llama.cpp
  • Qwen2.5-Coder 1.5B Instruct (Q4_K_M, 1.1 GB, Apache-2.0), from huggingface.co/Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF
    chosen for this machine (16 GiB RAM); others: ferric setup --model 0.5b | 1.5b | 7b | 14b
Each file is pinned and checked against its SHA-256 before use.
Engine → $HOME/.local/share/ferric/engine   Model → $HOME/.local/share/ferric/models
  engine verified in 1 s
  engine ready: $HOME/.local/share/ferric/engine/llama.cpp-b11780/llama-b11780/llama-server
  model verified in 41 s
  model ready: $HOME/.local/share/ferric/models/qwen2.5-coder-1.5b-instruct-q4_k_m.gguf
Ready. Run `ferric` in any project folder.
(exit 0)
$ ferric setup   # again
Ferric is ready: an engine is installed and $HOME/.local/share/ferric/models holds a model.
Run `ferric` in any project folder.
(exit 0)
$ ls ~/.local/share/ferric/engine ~/.local/share/ferric/models
$HOME/.local/share/ferric/engine:
downloads
engine.json
llama.cpp-b11780
$HOME/.local/share/ferric/models:
qwen2.5-coder-1.5b-instruct-q4_k_m.gguf
```

This transcript was captured before a wording fix: a model asked for with
`--model` is now described as "as requested" instead of "chosen for this
machine" (unit-tested in `setup_cmd`).

### A real answer from what setup installed

Ferric's own engine launch is refused by this sandbox's port-ownership
inspection, as on the untouched pre-sprint tree (see the test report and
T-12810). So the installed engine was started directly on the downloaded
model, and `ferric` used it as a configured endpoint (`api_base` and `model`
in the user config):

```
$ ~/.local/share/ferric/engine/llama.cpp-b11780/llama-b11780/llama-server -m ~/.local/share/ferric/models/qwen2.5-coder-1.5b-instruct-q4_k_m.gguf --host 127.0.0.1 --port 18080 -c 4096 &
$ ferric run "In one sentence, what does the Rust borrow checker do?"
Folder: $HOME/project
Checking the selected server model…
Ready: $HOME/.local/share/ferric/models/qwen2.5-coder-1.5b-instruct-q4_k_m.gguf (borrowed (left running on exit))
Ask only — no file changes. Type a question; /quit ends the session.
The Rust borrow checker ensures that data is used safely and does not cause data races or other concurrency issues.
Closing session…
```

## Installer integrity

The same mirror served a `.sha256` file of zeros. `install.sh` stopped with
`checksum mismatch … (expected 0000…, got 3985…)`, exited 1, and created no
install directory.

## The interactive first run

`ferric` ran under a pseudo-terminal with Enter as the answer. The empty home
had a placeholder GGUF in the models folder, so only the engine was missing
and no multi-gigabyte model download was needed. The engine came from the
release-API mirror.

```
$ ferric
Folder: $HOME/project
To run a model on this machine, Ferric needs:
  • the llama.cpp engine, release b11780 (llama-b11780-bin-ubuntu-x64.tar.gz), from github.com/ggml-org/llama.cpp
Each file is pinned and checked against its SHA-256 before use.
Engine → $HOME/.local/share/ferric/engine   Model → $HOME/.local/share/ferric/models
Download now? [Y/n]
  engine verified in 0 s
  engine ready: $HOME/.local/share/ferric/engine/llama.cpp-b11780/llama-b11780/llama-server
Ready.
The selected model changed or cannot be inspected. Select it again.
```

The session then went on to model selection, which refused the placeholder
file as expected: the offer is part of the normal `ferric` flow, not a
separate step.

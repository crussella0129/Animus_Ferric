# The Ferric testbench

Ferric's thesis is that behaviour should **scale to the model**: a big model just
works; a small model needs smaller steps. The testbench makes that observable —
it answers *"is this model good enough to drive the tools?"* with a per-tool
readout, so you can pick the smallest model that still works for your machine.

It's two commands: `ferric server` (launch the inference server) and
`ferric toolbench` (measure + diagnose).

## 1. Launch a server

The constrained-decoding path runs through an OpenAI-compatible server (the
ADR-001 HTTP valve). `ferric server` manages it for you — bound to `127.0.0.1`
only, never a public interface.

```sh
# llama.cpp (default engine): point at a GGUF. Add --mmproj for multimodal
# (image/audio/video) models like Gemma 3n.
ferric server up --engine llama-server --model path/to/model.gguf [--mmproj path/to/mmproj.gguf] [--ctx 8192] [--port 8080]

# Ollama (pluggable): the model is whatever you've `ollama pull`-ed.
ferric server up --engine ollama --model qwen2.5-coder:7b
```

`up` spawns the server, waits for it to start listening, and registers it in
`.ferric/server.json` (`{engine, pid, port, base_url}`). Other commands read that
file, so you don't repeat connection flags.

```sh
ferric server status   # is it up? prints the base URL + health endpoint
ferric server doctor    # checks the engine binary + model are present
ferric server down      # stops it and clears the runfile
```

## 2. Run the diagnostic toolbench

```sh
ferric toolbench --backend openai --model <name> --protocol grammar --iterations 20 --report report.md
```

- `--backend openai` with no `--api-base` auto-discovers the server from the runfile.
- `--protocol grammar` exercises the **constrained** path (the server enforces a
  JSON-Schema over the action space). Use `--protocol native` or `xml` to compare
  the unconstrained paths.
- `--report report.md` writes a Markdown report **and** a machine-readable
  `report.jsonl` (one row per tool + an `__overall__` row). Without it, the
  report just prints to stdout.

## 3. Read the verdict

Each tool gets a success rate and, when it misses, a **failure taxonomy**:

| Outcome | Meaning |
|---|---|
| `success` | called the right tool with all required args |
| `wrong_tool` | called a real but different tool |
| `malformed_args` | right tool, missing a required argument |
| `no_action` | produced no tool call at all (chatted instead) |
| `parse_error` | produced action-shaped text that didn't parse (under a constraint, this usually means the server isn't enforcing it) |

…and an acceptability **verdict band**:

- **solid** — ≥ 90 %
- **marginal** — ≥ 70 %
- **unreliable** — < 70 %

Now dial the model down (a smaller quant, fewer params) and re-run. When the
verdict slips from *solid* to *marginal* to *unreliable*, you've found the floor
for that machine. The `constrained` path should sit far higher than `native` on
small models — that's the whole point of harness-owned decoding.

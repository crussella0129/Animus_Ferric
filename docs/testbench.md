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

## 4. Calibrate the whole fleet at once

Rather than dialing one model down by hand, sweep a list in one shot with
`--models` (comma-separated — ollama model names, or GGUF files for the mistral
backend). Each model is benched in turn and the results collapse into a single
**leaderboard**, sorted best→worst:

```sh
# Every model shares the one running ollama server, so this is cheap:
ferric toolbench --backend openai \
  --models qwen2.5-coder:7b,llama3.1:8b,qwen2.5-coder:1.5b \
  --protocol grammar --iterations 20 --report fleet.md
```

```
# Fleet Leaderboard

| Model              | Protocol        | Success | Rate   | Verdict  |
|--------------------|-----------------|---------|--------|----------|
| qwen2.5-coder:7b   | ConstrainedJson | 100/100 | 100.0% | solid    |
| llama3.1:8b        | ConstrainedJson |  96/100 |  96.0% | solid    |
| qwen2.5-coder:1.5b | ConstrainedJson |  74/100 |  74.0% | marginal |
```

Read it top-down and **pick the smallest model still in the band you need** —
that's the "good enough on this machine" answer, found in one run. `--report`
also writes `fleet.jsonl`, every model's per-tool rows tagged by `model`. (This
is a human-facing readout; it does not change a model's stored `measured_level`
— that stays `ferric bench`'s job.)

## 5. Calibrate the rings — how far can this model go?

Tools are organized into **rings** (the always-on Ring-0 core, then wider rings
as a model proves itself). `--calibrate-rings` benches a model **ring by ring**
and tells you the largest ring it reliably drives — the recommended `--max-ring`:

```sh
ferric toolbench --backend openai --models qwen2.5-coder:7b,llama3.2:1b \
  --protocol grammar --iterations 20 --calibrate-rings --report calib.md
```

```
=== calibrating qwen2.5-coder:7b ===
  ring | tools |   rate | verdict
  -----|-------|--------|----------
     0 |     6 | 100.0% | solid
     1 |     8 | 100.0% | solid
  → Recommended --max-ring 1 (solid through ring 1)
```

It sweeps `ring 0, 1, …` until a ring stops being `solid` (≥90%), then reports
the highest ring with an unbroken solid run from the core. This is the
demonstrated-reliability promotion: a model *earns* a wider grammar by proving it
on the bench.

The sweep only reaches the rings the bench *tier* admits. `--params-b <N>`
(default 8.0 → Small → rings 0–1) sets that tier — `--params-b 20` benches at the
**Medium** ceiling (rings 0–2), so `--calibrate-rings --params-b 20` measures
whether a model can drive **Ring 2** (`multi_edit`) regardless of its nominal size.

### Make the promotion durable (`--profile-dir`)

Add `--profile-dir <dir>` (default `benchmarks`) and `--calibrate-rings` **persists**
each model's earned ring into `<dir>/model_profiles.json` (the same store
`ferric bench` writes `measured_level` to). Then `ferric query --profile-dir <dir>`
reads it back: a model with a recorded profile **automatically** runs at its earned
tier (`measured_level`) and ring (`calibrated_ring`) — no manual `--max-ring`:

```sh
# 1. Prove the model once — writes calibrated_ring into benchmarks/model_profiles.json
ferric toolbench --backend openai --models llama3.2:1b --protocol grammar \
  --calibrate-rings --profile-dir benchmarks

# 2. Every later query auto-applies it (the trace shows the capped ring)
ferric query --backend openai --model llama3.2:1b "refactor this module"
```

`measured_level` *raises* the tier (capability earned); `calibrated_ring` *caps*
the rings at what was proven (earned, not assumed). An explicit `--max-ring` still
overrides, and a model with no recorded profile runs exactly as before — the
read-back is a safe no-op until you've actually measured the model.

## 6. The full agentic loop — `ferric bench` (L0–L6)

The toolbench measures whether a model fires the *right single tool call*.
`ferric bench` runs the **whole multi-turn loop** against a ladder of real tasks
(L0 single readonly call → L6 a full todo app), and sets the model's
`measured_level` = the highest level it *completes* end-to-end:

```sh
ferric bench --backend openai --api-base http://localhost:11434/v1 \
  --model qwen2.5-coder:7b --params-b 7 --protocol grammar
```

```
L0 single-readonly-tool — PASS (2 turns, 70 tok)
...
L6 full-todo-app        — PASS (5 turns, 1110 tok)
calibrated qwen2.5-coder:7b: measured_level 6 (Small -> Large)
```

It writes `measured_level` into `benchmarks/model_profiles.json`, so a later
`ferric query --profile-dir benchmarks` auto-runs the model at its *earned* tier
(§5's read-back, now with full-loop data). `--backend openai` targets ollama or a
`ferric server`; `--backend mistral` uses a local GGUF (text-only — its constrained
path hangs upstream); `--mock` is the CI self-test. This is the end-to-end check
that the constrained loop *completes tasks*, not just that it emits tool calls.

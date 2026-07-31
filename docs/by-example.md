# Worked Examples

This is the gauntlet: complete, copy-pasteable runs that exercise every feature
covered earlier, from a first mock run to a full research-and-act loop. It is
meant to grow into the longest part of the book — a worked implementation for each
capability, basic and advanced, plus purpose-built server setups.

> **This chapter grows.** The examples below are real and runnable today. More
> will be added as features land and as specialized recipes are worked out; that
> expansion is stated intent, tracked here so it is not forgotten. Examples that
> depend on an unbuilt feature are marked.

Each example lists what it needs, the commands, and what you should see.

---

## 1. Zero to first run — no model

*Needs: a built `ferric` (any features).*

```sh
ferric query "do a mock task" --mock
```

Expected: a scripted file write, then a `task_complete`, ending with
`[task_complete after 2 turn(s); trace: .ferric/trace/q-….jsonl]`. Render the
trace:

```sh
ferric trace cat .ferric/trace/q-*.jsonl
```

---

## 2. A real query against a served model

*Needs: `--features backend-openai`, `llama-server`, a GGUF.*

```sh
ferric server up --engine llama-server --model ./model.gguf
ferric server status
ferric query "list the Rust files and summarize the largest one"
ferric server down
```

Expected: `query` auto-discovers the server from `.ferric/server.json`, runs a
constrained multi-turn loop, and prints a summary. No `--api-base` needed.

---

## 3. Talk, then act, in one session

*Needs: a served model.*

```sh
ferric chat
```

```
you> what would it take to add a --version flag here?
model> (a plain explanation — no tools ran)
you> /do add it
▸ calling edit_file...
✓ edit_file: added the flag
[task_complete after 2 turn(s); trace: …]
you> /exit
```

Expected: talk turns never touch the filesystem; only the `/do` turn runs the
agentic loop and writes a trace. See [Chatting, and `/do`](basics-chat.md).

---

## 4. Measure a model, then let it promote itself

*Needs: a served model.*

```sh
# Single-tool fire rate, ring by ring:
ferric bench ltd --model your-model --protocol grammar --calibrate-rings --profile-dir benchmarks
# Multi-turn task completion across L0–L6:
ferric bench full --model your-model --protocol grammar --results-dir benchmarks
# Now a normal query auto-runs at the earned tier + ring:
ferric query "refactor the config loader" --profile-dir benchmarks
```

Expected: `benchmarks/model_profiles.json` records `measured_level` and
`calibrated_ring`; the final query reads them back and runs at the *earned* tier
with no manual `--tier`/`--max-ring`. See [Tool Rings & Capability
Tiers](advanced-tool-rings.md) and [Benchmarking Your Model](testbench.md).

---

## 5. An edge-tuned server

*Needs: `llama-server`, a GGUF, a constrained machine (or just to try the flags).*

```sh
ferric server up --engine llama-server --model ./model.gguf \
  --ctx 8192 --threads 4 --gpu-layers 20 --batch-size 256
ferric server doctor --engine llama-server --model ./model.gguf
```

Expected: the launcher maps these to `-c/-t/-ngl/-b` on `llama-server`, bound to
`127.0.0.1`. `doctor` confirms the binary and model are present. See [The
Inference Server](server-configuration.md).

---

## 6. Reading an image

*Needs: a served model with a projector (`--mmproj`).*

```sh
ferric server up --engine llama-server --model ./vlm.gguf --mmproj ./mmproj.gguf
ferric query --file diagram.png --modality image "describe the diagram"
```

Expected: the image attaches as a content part (because `--modality image` is
declared and the backend carries media) and the model answers. See [Multimodal
Input](multimodal.md).

---

## 7. Research a workspace, quarantined

*Needs: a served model.*

```sh
ferric query "summarize how errors are handled here" --research "error handling"
```

Expected: Ferric searches the workspace, routes matches through the Ornstein
quarantine (a tools-free, memory-free, data-only summarizer), and folds the
provenance-tagged digest into the prompt. Because untrusted content entered the
run, later mutations are gated by the `--sink-action` policy. See
[Ornstein — Quarantined Research](ornstein.md).

---

## 8. Delegate across stages with ICM

*Needs: nothing for `init`/`plan`; a served model for `run`.*

```sh
ferric icm init ./pipeline          # scaffold a 3-stage workspace (offline)
ferric icm plan ./pipeline          # print the orchestration plan (offline)
ferric icm run ./pipeline           # execute the pipeline
```

Expected: `init` and `plan` are fully offline; `run` drives each stage as a
contained agent turn, the folder structure *being* the orchestration. See [ICM —
Agent Delegation](icm.md).

---

## 9. Expose Ferric to an IDE over MCP

*Needs: a served model, an MCP client.*

```sh
ferric mcp --workspace ./project --model your-model
```

Point an MCP-stdio client at that command; it sees one tool, `ferric_query`, that
runs a full contained Ferric turn. See [MCP & Animus Dark Matter](advanced-mcp.md).

---

## 10. Purpose-built recipes *(intent — growing)*

Planned additions to this gauntlet, recorded so they are not lost:

- A CPU-only Raspberry Pi setup end-to-end, with the tier the hardware actually
  earns.
- A fleet leaderboard sweep choosing the smallest "solid" model for a task.
- A scheduled agent via [`ferric cron`](cron.md).
- A resumable long run (`--resume`) after an interruption.
- Server recipes tuned per model class (small chat model vs. code model vs. VLM).

These are intended examples, not yet written.

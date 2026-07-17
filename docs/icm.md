# ICM — Interpretable Context Methodology (agent delegation)

Ferric's answer to "agent delegation" is **not** a multi-agent framework. It is
**Interpretable Context Methodology (ICM)** — the filesystem *is* the
orchestration layer. This follows the paper *"Interpretable Context Methodology:
Folder Structure as Agent Architecture"* (Van Clief & McDermott, 2026), adapted
to Ferric's harness-owns-decoding, workspace-contained model.

> The central observation: if the prompts and context for each stage of a
> workflow already exist as files in a well-organized folder hierarchy, you do
> not need a coordination framework to manage multiple specialized agents. You
> need one orchestrating agent that reads the right files at the right moment.

## Why ICM (and not CrewAI/LangChain/AutoGen)

Ferric targets **sequential, human-reviewed workflows on small local models**.
For that shape, a code-level orchestration framework solves a coordination
problem that need not exist. ICM replaces framework code with folder structure:

| Operation | Framework | ICM |
|---|---|---|
| Change stage order | edit orchestration code, redeploy | rename/reorder folders |
| Modify a prompt | edit agent config in code | edit a markdown file |
| Add/remove a stage | write an agent class | add/delete a folder |
| Inspect intermediate state | add logging, build a dashboard | open the folder, read the files |
| Hand off to someone | document env + deps + setup | copy the folder |

It also keeps each stage's context **small and focused** — the "lost in the
middle" degradation that hits a monolithic 40k-token prompt never happens,
because the folder structure loads only the files the current stage needs.

## The five-layer context hierarchy

A stage's agent sees a scoped slice of context, assembled from five layers:

| Layer | File(s) | Question | Stable? |
|---|---|---|---|
| **0** Identity | `Animus.md` (or `CLAUDE.md`) | "Where am I?" | yes |
| **1** Routing | `CONTEXT.md` (workspace root) | "Where do I go?" | yes |
| **2** Contract | `stages/NN_*/CONTEXT.md` | "What do I do?" | per stage |
| **3** Reference | `references/`, `_config/` — the *factory* | "What rules apply?" | yes (across runs) |
| **4** Working | a prior stage's `output/` — the *product* | "What am I working with?" | no (per run) |

Layer 3 is **internalized as constraints** (voice, conventions); Layer 4 is
**processed as input** (the previous stage's artifact). Separating them in the
folder structure means the model receives already-organized context instead of
sorting persistent rules from per-run material itself.

## Workspace layout

```
workspace/
  Animus.md                    Layer 0 — identity
  CONTEXT.md                   Layer 1 — routing
  stages/
    01_research/
      CONTEXT.md               Layer 2 — contract
      references/              Layer 3
      output/                  Layer 4 (this stage's product)
    02_script/
      CONTEXT.md
      references/
      output/
    03_production/
      CONTEXT.md
      references/
      output/
  _config/                     Layer 3 — shared factory config
  shared/                      Layer 3 — shared reference
```

The **numbering encodes execution order**. Stage `02`'s output is stage `03`'s
input. If a human edits `01_research/output/` before stage 2 runs, stage 2 picks
up the edited version — every output is an edit surface.

## Stage contracts

Each `stages/NN_*/CONTEXT.md` is a three-part contract:

```markdown
## Inputs
- Layer 4 (working): ../01_research/output/
- Layer 3 (reference): ../../_config/voice.md

## Process
Write a script based on the research output.
Match the tone described in voice.md.

## Outputs
- script_draft.md -> output/
```

The **Inputs** table is the control point: it names exactly which Layer 3/4 files
the stage agent loads, making context selection explicit, editable, and
auditable. The parser is tolerant (the file is a human edit surface): sections
are case-insensitive, an unlabeled input defaults to the reference layer, and an
output with no `->` defaults to `output/`.

## Ferric-native security

ICM does not weaken Ferric's guarantees. Each stage is a **workspace-scoped
run**, so `ferric-guard`'s containment (ADR-005) applies unchanged: every path a
contract references is resolved through the workspace boundary, so a contract
**cannot pull context from outside the workspace** — a `../../../../etc/passwd`
input is refused, not read. (Externally-*sourced* Layer 4 content — a research
stage that fetches the web — routes through Ornstein's quarantine; that
composition lands in a later increment.)

## Using it

```sh
# Scaffold a new ICM workspace (LLM-free, refuse-to-clobber):
ferric icm init ./my-workspace

# Inspect the orchestration plan — which files, at which layers, each
# stage-agent would receive. No model runs; this is the delegation logic
# made inspectable.
ferric icm plan ./my-workspace
ferric icm plan ./my-workspace --show-context   # also print each composed prompt

# Run the pipeline. Each stage's composed context is fed into the same
# constrained agent loop `ferric query` drives, in numeric order.
ferric icm run ./my-workspace                    # pauses for review between stages
ferric icm run ./my-workspace --auto             # run straight through, no gates
ferric icm run ./my-workspace --from 2 --to 2    # run only stage 2
ferric icm run ./my-workspace --auto --mock      # offline dry-run of the wiring
```

A `!` in the plan marks a declared input that is absent (e.g. an upstream stage
has not run yet) — expected before a run, not an error.

### How `run` works

For each stage in the chosen range, `ferric icm run`:

1. Composes the stage's scoped context (the same `OrchestrationPlan` `plan`
   prints), including the Outputs directive telling the agent where to write.
2. Runs it through the constrained agent loop (`run_with_provider` — the exact
   path `ferric query` uses: guard, loop guards, JSONL trace), with the stage's
   **own folder** as the workspace boundary.
3. Checks the terminator. A successful stop (`task_complete` / `submit_plan` /
   final text) continues the pipeline; any other stop (max turns, provider error,
   guard trip) **halts** it — a downstream stage must not read untrustworthy
   output.
4. Unless `--auto`, pauses at a **review gate** before the next stage: you edit
   the output on disk, then press Enter to continue or `q` to stop. The next
   stage reads whatever you left there.

Each stage writes one trace to `<workspace>/.ferric/trace/icm-<stage>-<ts>.jsonl`.

**Containment (stronger than the paper).** Each stage runs bounded to its own
`stages/NN_*/` folder, so a stage can only write inside its own directory — it
cannot clobber a sibling stage or the shared config. This works because the
composed context already folds the prior stage's output in as Layer 4, so a stage
never needs cross-stage filesystem *reads*; cross-stage data flows only through
the composed context and the review gate.

## Increments

- **Increment 1 (sprint 73, ADR-064) — the model + inspection + scaffold.** The
  `ferric-icm` crate: discover a workspace, parse contracts, compose each stage's
  scoped context (guard-checked) into an `OrchestrationPlan`, and scaffold a new
  workspace skeleton. Surfaced via `ferric icm init` / `ferric icm plan`.
- **Increment 2 (sprint 74, ADR-065) — live execution.** `ferric icm run`
  executes the pipeline stage by stage through the constrained loop, each stage
  contained to its own folder, with halt-on-failure and human review gates
  (`--auto`, `--from`/`--to`, `--mock`). The delegation actually runs.
- **Later (deferred).** Ornstein-quarantining of externally-sourced Layer 4
  content (a web-fetching research stage); a GECK-style workspace-builder;
  conditional/branch routing (ICM is sequential by design).

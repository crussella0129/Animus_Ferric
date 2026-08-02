# Tool Rings & Capability Tiers

The tool ring model is the most load-bearing idea in Animus after harness-owned
decoding itself. It is how a small model is given *exactly* the tool vocabulary it
can drive reliably — no more — and how that vocabulary widens only as the model
earns it.

## The problem it solves

Give a model a tool it cannot reliably call and you do not get a slightly worse
agent; you get a stuck one — it reaches for the tool, malforms the call, and
burns turns. The fix is not "more prompting." It is to **not offer** tools the
model has not proven it can use. The offered tools *are* the constrained grammar,
so restricting them is restricting what the model can even emit.

## The rings

Tools are organized into concentric rings that widen as reliability is proven:

- **Ring 0 — the navigate/mutate core** (always on): `read_file`, `list_dir`,
  `write_file`, `make_dir`, `edit_file`, `delete_path`, `search_files`,
  `move_path`, and `copy_file`. This is the smallest, surest grammar.
- **Ring 1 — "find & inspect history"**: `find_files` and read-only `git_read`.
- **Ring 2 — "plan & apply structured changes"**: `multi_edit` (an ordered,
  *atomic* batch of edits to one file), `apply_patch` (a context-located unified
  diff), and `git_write`.

`shell_exec` and `manage_task` are not model tools at any ring. They run on the
host rather than in an OS sandbox, so they exist only in the dedicated
human-command registry used by interactive chat's explicit `!cmd`/`/run` path.

Outer rings assume the model can already drive the inner ones; the loop **trims
from the outer ring first**, so the core is never dropped.

## Tiers

A model runs at a **tier** — `Nano`, `Small`, `Medium`, `Large`, `Xl`, `Ultra` —
which sets the ceiling on how wide the rings go, plus the turn and tool-call
budgets. Tier is derived, in order of authority:

1. an explicit `--tier` (operator override, recorded as such);
2. a persisted `measured_level` from benchmarking (the *earned* tier);
3. otherwise a prior from the parameter count (`--params-b`).

The key finding behind all of this: **single-tool-call reliability is not agentic
capability.** A 1B model can fire a correct single call at 100% and still be unable
to complete a multi-turn task. Tiers are set by measured multi-turn completion, not
by size or by single-shot fire rate.

## Earning a wider grammar

You do not have to trust the defaults. Prove a model and let it promote itself:

```sh
# Measure single-tool fire rate ring-by-ring and report the highest ring it
# reliably drives (the recommended --max-ring):
ferric bench ltd --model <name> --protocol grammar --calibrate-rings --profile-dir benchmarks

# Measure multi-turn task completion across the L0–L6 ladder -> measured_level:
ferric bench full --model <name> --protocol grammar --results-dir benchmarks
```

These write `benchmarks/model_profiles.json`. A later `ferric query --profile-dir
benchmarks` **reads that profile back** and auto-runs the model at its earned tier
(`measured_level`) and calibrated ring — no manual flag. See
[Benchmarking Your Model](testbench.md) for the full workflow.

## Controlling rings directly

```sh
ferric query "…" --max-ring 0   # pin any model to the Ring-0 core, whatever its size
```

`--max-ring` is **restrict-only**: it can narrow the grammar below the tier
ceiling, but it cannot widen a model past what it has earned. To widen, you
benchmark. This asymmetry is deliberate — capability is demonstrated, never
asserted.

## Guards that bound the waste

Because a model *can* be handed a ceiling it cannot fully use, a family of
loop-hardening guards bounds the cost of a model that gets stuck: the **repetition
guard** (identical calls), the **no-progress guard** (same tool name, different
args, no progress), and the **repeated-failure guard** (consecutive all-errored
turns). They compose by threshold and stop a stuck run early with a precise reason
(`no_progress`, `repeated_failure`) instead of grinding to `max_turns`. They bound
wasted compute and sharpen diagnostics — they do not lift a capability ceiling.

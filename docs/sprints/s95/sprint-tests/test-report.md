# Sprint 95 — Test Report

## Gate

| Check | Result |
|---|---|
| `cargo test --workspace` | **549 passed / 0 failed** |
| clippy / fmt | 0 warnings / clean |

## The refreshed fleet map

llama.cpp, `--protocol grammar`, release build, ctx 8192, `--gpu-layers 99`.

| model | L0 | L1 | L2 | L3 | L4 | L5 | L6 | measured_level | tier |
|---|---|---|---|---|---|---|---|---|---|
| `qwen2.5-coder-3b` | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ | ❌ | **4** | Small |
| `qwen2.5-coder-7b` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | **6** | Large |

Same family, so the difference is **size**. The 7B's 6 matches its sprint-20
figure — the number survived 75 sprints of harness change.

Wall clock: 3B ladder 3m04s, 7B ladder 5m32s.

## H1 — a partial sweep downgraded a stored profile

`bench full --level 5` rewrote the 7B's record from `measured_level 6 (Large)` to
`5 (Medium)`, because `calibrate()` sees only the current sweep's rows.

That is not cosmetic: `ferric query` reads this profile to set the tier
(ADR-029), so **a diagnostic command shrank the model's RunPolicy** — fewer
tools, fewer turns, no subagents.

Fixed: a profile is written only from a full ladder; a partial sweep prints
`profile left unchanged` and leaves the record alone. Verified live.

## H2 — a non-monotonic ladder was silently flattened

The 7B's first sweep **failed L5 and passed L6** → scored 6, output
indistinguishable from a clean run.

Evidence it was noise, not capability:

| run | L5 |
|---|---|
| full sweep #1 | FAIL |
| `--level 5` repeat | PASS |
| `--level 5` repeat | PASS |
| `--level 5` repeat | PASS |
| full sweep #2 | PASS (all seven levels) |

So the figure was right and the *reporting* was wrong. With one sample per level,
`max(passed)` means luck only ever inflates. The calibrator now names levels that
failed below the highest pass. 4 unit tests, including that failures **above** the
top pass are an ordinary ceiling and must not be flagged.

## Operational note

The sprint's first bench attempt failed every level in ~200 ms with 0 turns.
Cause: `cargo test --workspace` had rebuilt `ferric.exe` **without**
`--features backend-openai`, so the spawned child had no backend. Rebuild with
the feature and `--release` before a sweep — the harness warns that debug runs at
~1 tok/s.

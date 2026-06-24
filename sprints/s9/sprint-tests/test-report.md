# Sprint 9 Test Report — Fleet Calibration

**Date:** 2026-06-23 · **Engine:** ollama 0.30.10 (`:11434`) · **Binary:** `ferric-cli --features backend-openai` (debug).
Models: `qwen2.5-coder:7b`, `llama3.1:8b`, `llama3.2:1b` (pulled this run for the dial-down floor).

## Unit + Integration (default CI — all green)
- `cargo test --workspace` — green across all crates.
- **T-901** `toolbench_cmd::tests::leaderboard_sorts_best_first` — leaderboard sorts best→worst, shows all three verdict bands. ✓
- **T-901** `render_report` / `summary_rows` updated for the `model` field. ✓
- **T-902** `openai::tests::toolcall_from_content_{ollama_shape, harness_shape, args_as_json_string, prose_is_none}` — all ✓ (requires `--features backend-openai`).
- fmt clean; clippy `-D warnings` clean (ferric-cli openai + ferric-provider openai).

## E2E — the fleet calibration (the headline artifact)
One command, three models, both protocols, 5 tools × 10 iterations each:

### Constrained (`--protocol grammar`, ConstrainedJson)
| Model | Success | Rate | Verdict |
|-------|---------|------|---------|
| qwen2.5-coder:7b | 50/50 | 100.0% | **solid** |
| llama3.1:8b | 50/50 | 100.0% | **solid** |
| llama3.2:1b | 50/50 | 100.0% | **solid** |

### Native (`--protocol native`, NativeTools — *with* the T-902 fallback)
| Model | Success | Rate | Verdict |
|-------|---------|------|---------|
| qwen2.5-coder:7b | 50/50 | 100.0% | solid |
| llama3.1:8b | 50/50 | 100.0% | solid |
| **llama3.2:1b** | **11/50** | **22.0%** | **unreliable** |

## The thesis, demonstrated end-to-end
The constraint **extends the usable model floor down to 1B**: ConstrainedJson holds
at 100% on `llama3.2:1b`, exactly where the native path collapses to 22%. This is
the whole argument for harness-owned decoding, now shown on a real model fleet with
one command.

**The diagnosis (toolbench earning its keep again).** The 1B native failures are
**`malformed_args`, not `no_action`** — per-tool from `fleet_native.jsonl`:

| tool | native 1B | failure mode |
|---|---|---|
| move_path | 10/10 | — (solid) |
| write_file | 1/10 | `malformed_args×9` |
| list_dir / make_dir / read_file | 0/10 each | `malformed_args×10` |

Two findings fall out of that taxonomy:
1. **The T-902 fallback works.** The outcome is `malformed_args` (right tool, recovered
   from `content`), *not* `no_action` — so the synthesized-from-text tool call landed;
   the 1B model simply omits required arguments when unconstrained.
2. **Why the constraint wins.** ConstrainedJson forces the full action schema —
   *including* `required` args — so the same 1B model that drops args natively is held
   to 100% under the constraint.

## T-902 before/after (ADR-024)
Sprint 8 measured native at **0%** on ollama (it returns the call as text, `tool_calls`
null → `no_action`). With the fallback, native is **100%** on the 7–8B models and a
*diagnosable* 22% on 1B (`malformed_args`, not a silent zero). The ADR-024 gap is closed.

## mistral.rs 0.8.15 viability probe (ADR-023/024 gate)
`grammar_probe` against **mistralrs 0.8.15** (git master `80fdfbc`) on Llama-3.2-1B,
bounded subprocesses:

| Variant | Schema | Result |
|---|---|---|
| `trivial` | `{x: string, required:[x]}` | **PROBE RETURNED in 10.7s** (no hang) |
| `unified` | the real action `anyOf` (the exact ADR-020 hang trigger) | **PROBE RETURNED in 10.7s** (no hang) |

Two findings:
1. **The ADR-020 hang is fixed.** Both variants — including `unified`, the schema
   that hung unboundedly in s2 — now return in ~10s. The upstream constrained-decoding
   hang is gone in 0.8.15.
2. **…but the constraint is NOT enforced.** Both probes returned the *identical*
   freeform JSON (a "John Doe" object wrapped in a ```json fence) — matching **neither**
   schema (`{x:string}` nor the action `anyOf`). Identical output across two different
   schemas = the `JsonSchema` constraint has zero effect on generation; mistral.rs is
   doing an unconstrained completion.

**Verdict:** mistral.rs 0.8.15 no longer hangs, but its constrained path is still
non-functional (constraint ignored), so it **cannot** be the pure-Rust constrained
backend. It stays the unconstrained **TextXml** fallback; the constrained thesis stays
on the HTTP valve (llama.cpp/ollama), which the fleet calibration above just proved at
100% down to 1B. (Whether the gap is upstream or in our `MistralRsProvider` wiring is a
future investigation, not a blocker — the valve is the workhorse per ADR-001/023.)

## Artifacts
`fleet_calibration.md`/`.jsonl` (constrained), `fleet_native.md`/`.jsonl` (native) in
the repo root (gitignored `report*`/`fleet*` run artifacts; the numbers are captured here).

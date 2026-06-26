# Sprint 21 Research Report — Fleet agentic capability map (`bench --models`)

> Sprint 20 proved the full multi-turn loop works on the constrained backend and
> that **qwen2.5-coder:7b maxes L0–L6** (measured_level 6). Open question before
> designing harder levels: **how do the *other* fleet models do?** Can a 1B
> *complete* multi-turn tasks, or only fire single tool calls (toolbench)? Run the
> now-working full-loop bench across the fleet and map each model's agentic
> `measured_level`. The result also tells us whether harder levels (L7+) are even
> needed yet (if the 1B/8B don't saturate, the ladder still discriminates).

## Decisions Reviewed
- **ADR-030** (sprint 20) — `bench --backend openai` reaches the constrained backend; the full loop is validated. The fleet sweep reuses that path per model.
- **ADR-019** — `ferric bench` is the SOLE producer of `measured_level`; a fleet sweep writes one record per model (keyed by model id, like today).
- **ADR-008** — deterministic, sorted output (a model→measured_level leaderboard).
- **(toolbench fleet, sprint 9)** — the `--models` comma-sweep + leaderboard pattern to mirror.

## Grounding (read the code)
`run_bench` (`bench_cmd.rs`) already: resolves a backend → `Invocation`, runs
`for spec in &selected { run_spec; parse_trace; verify; append_row; print PASS/FAIL }`
(lines 159–231), then `calibrate(model_name, …)` + `write_profile`. The per-level
loop is self-contained — extract it as `run_levels(&selected, &inv, &args, model_label)
-> (Vec<ResultRow>, bool)` and call it from both the single path and a new fleet loop.
`ModelProfileRecord`/`calibrate`/`write_profile` already key by model id, so N models
→ N records with no store change.

## Design (settled)
- **`bench --models <a,b,c>`** (openai-only — the fleet case is ollama model ids). When set, for each model id: build an openai `Invocation` (`OpenAiArgs{api_base, model, params_b, ctx}`), `run_levels`, `calibrate` + `write_profile`, collect `(model, measured_level, tier_from_measured)`.
- **Extraction:** `run_levels` deduplicates the loop; the single-model path (mock/mistral/openai) calls it unchanged.
- **Leaderboard:** after the sweep, print `model | measured_level | tier` sorted by level desc (ADR-008). Exit non-zero only on a runner error, not on a low level (a low measured_level is a valid measurement).

## Risks
- **Runtime** — N models × L0–L6 multi-turn; run in the background. ollama keeps each model resident.
- **The 1B may complete few levels** — that *is* the finding (its agentic ceiling vs its 100% single-tool-call rate). Either way it's recorded honestly and answers the harder-levels question.
- **Refactor** — extracting `run_levels` touches one function; the single path must stay byte-identical (verified by `bench_mock`/`l0_smoke`).

## Recommended approach
T-2101: extract `run_levels` + add `bench --models` openai fleet sweep + a sorted
leaderboard; keep the single path + `--mock` ladder green. T-2102: run the fleet
(qwen2.5-coder:7b, llama3.1:8b, llama3.2:1b) → the agentic capability map + docs +
an ADR-030 amendment; note whether harder levels are warranted. AI-verifiable via
the `--mock`/`l0_smoke` regression; the live fleet run is the capability map.

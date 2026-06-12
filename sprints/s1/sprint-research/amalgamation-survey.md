# Artifact: Amalgamation Candidates Survey (oovra / GECK / sprint-loops)

> Source: Explore agent over the three local repos, 2026-06-10.

| Candidate | Current form | LOC | License | Verdict | Rationale |
|---|---|---|---|---|---|
| **oovra** | Rust workspace (lib + CLI + GUI) | 4,021 | MIT/Apache-2.0 | **Versioned crate dependency** | Mature lib API (46 tests), prompt-composition abstraction is a precision fit for per-tier/per-protocol prompt assembly under RunPolicy. Low effort: depend on the lib, ignore CLI/GUI/Migrate. |
| **GECK** | Python (tkinter GUI + CLI, Jinja2) | 4,107 | GPLv3 | **Absorb as Ferric bootstrap capability** | Language mismatch; nature is lightweight (21 project profiles + templates + env detection). Rust rewrite as `ferric init-project --profile X` (askama or static templates). GPLv3 applies to the Python original only; clean-room Rust rewrite is standalone. |
| **sprint-loops** | Bash scripts (~400 LOC) + 9 markdown particles; filesystem state machine | 1,802 | MIT | **Rust-native port → new `ferric-engine` crate** | The protocol is pure filesystem + deterministic routing — ideal Rust port. This IS the "Development Engine" mode: five phases driven by Ferric's local models under RunPolicy, confidence throttle feeding back into policy scaling. Highest ROI, ~4 sprints. |

## oovra details
Core abstractions: `PromptElement` (TOML frontmatter + markdown body), `Library` (HashMap by id, recursive dir load, duplicate detection), operators Create/Compose/Decompose/Compare/Migrate; compounds are losslessly decomposable with level-aware delimiters. Load-bearing: `src/element.rs` (627), `src/library.rs` (436), `src/render.rs` (252), `src/diff.rs` (243), `src/main.rs` (1048). Ferric use: `compose_prompt(tier, protocol) → String`, prompt genealogy recorded in trace events. Integration effort: LOW (path dep now, crates.io publish later).

## GECK details
Generates LLM_init.md + GECK folder structure (log.md, tasks.md, env.md) from 21 preset profiles + 6 "Repor" exploration profiles; pure templating, no LLM calls. Load-bearing: `core/profiles.py` (35KB), `core/generator.py`, `core/templates.py`. User's instinct ("could be an oovra library") is half-right: the *profiles* become oovra-compatible prompt elements; the *generator* becomes a Ferric subcommand. Effort: MEDIUM (1 sprint).

## sprint-loops details
Five phases (Research→Plan→Build→Test→Loop), exit artifacts on disk, `current-phase.sh` routing, plan/test critics, confidence throttle (Kalman-style ±0.1/−0.3, <0.5 → ≤5 tasks). Three adapters (open-harnesses canonical, claude-code, codex-cli) over one filesystem contract. Ferric port: `ferric-engine` crate implementing phase routing natively, phase *content* (particles) stays markdown (potentially oovra-composed), every phase transition a trace event, confidence gates RunPolicy mutation. Effort: HIGH (~4 sprints), prerequisite: production agent loop exists.

## Sequencing recommendation
s2: oovra dep + prompt assembly. s3: GECK absorption (`ferric init-project`). s4–s7: ferric-engine port. (s1 stays focused on the inference backend + loop.)

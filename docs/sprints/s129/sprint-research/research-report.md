# Sprint 129 Research Report — Redundancy analysis swarm

## Method

Eight read-only analysts each covered one slice of the workspace (140,082
lines of Rust in 20 crates at the start of the sprint). They shared one brief:

- Find six kinds of redundancy: duplicate logic, dead code, overlapping
  modules or crates, over-abstraction, redundant tests and stale scaffolding.
- Back every claim with file:line evidence, and confirm "unused" or
  "duplicated" by searching the whole workspace, including tests, examples,
  binaries and CI.
- Propose the single surviving version, and label anything user-visible as a
  RETIREMENT that needs owner approval (CLAUDE.md: retiring a standalone
  surface needs its own owner-approved intent).

| Slice | Code | Findings | Estimate, no retirement | Retirement candidates |
|---|---|---|---|---|
| ARCH: crate graph, manifests, tooling | R-ARCH-1..14 | 14 | about −690 lines, 20 → 14 crates | backend-free build, `tools/install.*` |
| SRV: `ferric server`, registrations, Tailscale | R-SRV-1..16 | 16 | about −2,100 | Tailscale Serve, schema-1 and `server adopt` |
| ENG: engine launch, processes, ferric-setup | R-ENG-1..13 | 13 | about −1,600 | none |
| CLI: query, chat, mcp, api, icm, tests | R-CLI-1..11 | 11 | about −1,500 | none |
| BENCH: ferric-bench, autonomy, toolbench | R-BENCH-1..16 | 16 | about −1,200 | paired Legacy-vs-Evidence autonomy |
| LOOP: ferric-loop | R-LOOP-1..17 | 17 | about −1,250 | EvidencePlanner arm, pre-sprint-113 traces |
| TOOLS: ferric-tools, ferric-guard, ferric-gate | R-TOOLS-1..17 | 17 | about −1,400 | the Legacy `Tool::run` path |
| MODEL: core, iron, provider, valve, prompt, trace, research | R-MODEL-1..15 | 15 | about −1,800 | tailnet research retriever |

The analysts overlapped in a few places, and each overlap is counted once in
the build plan:

- Iron re-export shims: R-ARCH-4 = R-LOOP-9 = R-MODEL-2.
- The provider's sprint-0 loop test: R-ARCH-3 = R-MODEL-5.
- The CI `backend-check` job: R-ARCH-10 = R-MODEL-14.
- SHA-256 helpers: R-ARCH-12, plus cross-area notes from BENCH, LOOP and
  MODEL.
- No-follow file identity code: R-ARCH-6 = R-TOOLS-4, plus R-ENG-8.
- Per-user directory resolvers: R-ARCH-14 = R-TOOLS-13.

## Answers to the owner's question: are core, iron and the valve redundant?

- **ferric-core and ferric-iron are not duplicates.** Iron depends on core in
  one direction, and iron's dependency-boundary test already counts core as
  inside the boundary. The redundancy is the compatibility re-exports left in
  ferric-loop and ferric-provider after the T-12601 move into iron. They are
  removed in this sprint. Core should shrink over time, as INT-0011 plans,
  rather than be merged now.
- **ferric-provider and ferric-valve do different jobs.** The provider is the
  client that turns Ferric messages into OpenAI requests. The valve is the
  OpenAI-compatible proxy that Hermes calls. They duplicate about 150 lines of
  pure OpenAI wire shapes: SSE line decoding, the `response_format` body,
  tool-call JSON and usage parsing. That shared part belongs in ferric-iron,
  which both already depend on. The HTTP clients stay in each crate because
  iron may not depend on reqwest (INT-0011 AC-1).
- **The disjointed pieces are mostly at the edges:**
  - Six leaf crates have one consumer each: ferric-vcs, ferric-cron,
    ferric-skills, ferric-prompt, ferric-icm and animus-launch.
  - A second CLI build without the backend is kept alive by CI.
  - Several standalone features, all superseded or abandoned, carry far more
    code than the product core:
    - Tailscale Serve publication: about 11,500 lines.
    - Schema-1 compatibility and `server adopt`: about 2,600 lines.
    - The paired Legacy-vs-Evidence autonomy runner: about 2,500 lines.

## Defects found while analysing (not redundancy)

| ID | Defect | Where |
|---|---|---|
| D-1 | Windows startup treated a transient `STATUS_PARTIAL_COPY` while reading a live engine's command line as fatal (CI failure on the dependency-update commit) | `ferric-cli/src/server_process.rs` |
| D-2 | `git_read` admits `git branch <name>`, which creates a branch, under Read permission | `ferric-tools/src/builtin/git_read.rs` |
| D-3 | `ferric server up` and `server doctor` look for `llama-server` on PATH only, so they cannot find the engine `ferric setup` installs | `ferric-cli/src/server/mod.rs` |
| D-4 | The provider ignores an in-band `{"error":…}` SSE event; the valve handles it | `ferric-provider/src/openai.rs` |
| D-5 | Lost line continuations leave runs of spaces inside model-facing and error text | `ferric-loop/src/projector.rs`, `ferric-provider/src/openai.rs` |
| D-6 | A concrete tailnet address in a tracked document (template hygiene) | `docs/work/tasks.md` |
| D-7 | `ferric explain` lists models from `<workspace>/models` only, not the directory `ferric run` resolves | `ferric-cli/src/startup.rs` |
| D-8 | After a truncated turn, live and resumed runs disagree on whether the next empty completion is nudged | `ferric-loop/src/run.rs`, `projector.rs` |
| D-9 | `icm init` checks clobber safety with `exists()` and `is_dir()`, which follow symlinks; `animus-launch` does not | `ferric-icm/src/lib.rs` |
| D-10 | `MIN_TAINT_SEGMENT_CHARS` is documented as a minimum but nothing enforces it | `ferric-guard/src/sink.rs` |

## Source reports

The eight full reports were delivered in-session. Their findings, with
evidence references, are indexed in the [build plan](../sprint-plans/build-plan.md)
by finding ID.

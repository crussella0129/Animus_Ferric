# Agent Tasks (Persistent Backlog)

> **Direction change in s30:** began the **Animus** suite by **hardening Animus Loop**.
> Sprint 30 shipped **Ornstein increment 1** — the quarantined summarizer (`ferric-research`):
> untrusted content → a no-tools/no-memory model under a data-only schema → a typed,
> provenance-tagged `ResearchDigest`. The quarantine is *structural* (reuses the constrained
> valve), so injections can only surface as quoted data. 4 tests incl. the injection-
> containment proof. ADR-040; `docs/ornstein.md`. PR cadence clean.

## Animus vision (recovered from the user, 2026-06-27; expanded 2026-06-29)
- **Animus Launch** — interactive Rust+scripts project bootstrapper (successor to GECK's launcher: `crates/geck-cli/wizard.rs` + `geck_generator`). Interview → git repo with main+dev → "begin work?". **Decision: lives as a crate in Animus_Ferric.**
- **Animus Loop** — the sprint-loops protocol (Research→Plan→Build→Test→Loop). **First priority = harden it.**
- **Animus Manage** — multi-agent project-management layer (least-specified).
- **Animus Beast-Zoo** (2026-06-29, **separate future repo**) — a safetensor→GGUF customizable fine-tune/conversion pipeline (HF fine-tunes, RAG/LoRA pipelines, eventually a visual "snap modules + sliders" app). Brief seed spec at `docs/beast-zoo-spec.md`; meant to be fed through its own future agent loop, not built here. User has a mixed GGUF+safetensors model library on a NAS (`192.168.86.27`, `Y:\models`) as a future test corpus.
- **(unnamed) native Rust inference engine** (2026-06-29, **separate future repo**) — long-term replacement for llama.cpp as Animus_Ferric's inference backend. Not started, no spec — recorded only.
- **Animus IDE** (2026-06-29, **separate future "organ"**) — an editor/IDE that talks to Ferric. Motivated the ADR-011 revision below.

**Model-format decision (2026-06-29): Animus_Ferric is GGUF-only, permanently.** The user
considered direct safetensors support in Ferric and decided against it for simplicity —
cross-format loading is pushed to the separate Beast-Zoo repo instead. **Do not revisit
multi-format model loading inside Ferric.**

## ADR-011 revision decided (2026-06-29, mid-sprint-35-research) — Ferric gets MCP + a genuine chat mode
The user deliberately revisited ADR-011 ("no REPL/chat mode") based on hands-on tool
experience since writing it — driven by **Animus IDE** needing to send one-off natural-language
change requests conversationally. **Both** of the following, not either/or:
1. **`ferric mcp`** — activates the already-planned ADR-012 MCP-stdio surface (deferred since
   s2–s3, blocked on "an ADR-005 security call" — do that call as part of this work). Each MCP
   message still runs a full constrained agentic query; no departure from "harness owns decoding."
2. **A genuine raw chat mode** — an actual unconstrained conversational surface. This is the
   literal reversal of ADR-011's "no REPL/chat" clause and touches the "never raw unconstrained
   chat" security thesis directly — needs its **own dedicated ADR** (not a quiet amendment)
   spelling out the security boundary (what a chat turn can/cannot do to the workspace).
Distinct from two prior, DIFFERENT "chat" rejections (don't conflate): ADR-011's original
REPL-as-alternative-to-query rejection, and the separate sprint-25 `--chat` *capability fallback*
for models too weak to agent (dropped once Gemma 4 E4B proved unnecessary).
**Decided (2026-06-29, s35 plan phase): deferred to sprint 36+, not built in s35** — both are
security-sensitive enough to deserve their own focused design sprint(s) rather than a tack-on to
the s35 audit/refactor sprint. See the s35 completion note below.

## Ornstein = a quarantined MULTI-SOURCE research subsystem (user's expanded vision)
One funnel (the built quarantine), many pluggable `Retriever`s (capability-probed). **Build order (user-chosen):**
- **inc 2 — `Retriever` trait + Local FS retriever** ✅ **DONE (s31, ADR-041).** Keystone `Retriever` (plane/available/retrieve, async) + `LocalFsRetriever` + `research()` pipeline; source→funnel→digest proven.
- **inc 3 — Tailnet/NAS FS retriever** ✅ **DONE (s32, ADR-042).** `TailnetFsRetriever` searches a remote tailnet device's FS over SSH (`SshTransport::{Tailscale, Plain{port}}`); query single-quote-escaped vs remote command injection; `parse_status_devices` for `available()`. Deterministic core tested; **live SSH E2E deferred** (no target's sshd was up — Pixel has none on :22/:8022, switchblade offline).
- **inc 3b — live SSH E2E for the tailnet plane** ← run once a target's sshd is up (Termux `Plain{8022}` on pixel-10-pro-xl, or `Tailscale` on switchblade when back online): `research(&TailnetFsRetriever{…}, provider, query)` → quarantined `host:path` digests.
- **research orchestrator** ✅ **DONE (s33, ADR-043).** `research_all(planes, provider, query) -> MultiResearch` runs a query across all available planes, chunk-level dedup by source (one model call per source), per-plane `PlaneResult` report; unavailable planes are recorded no-ops.
- **inc 4 — Web retriever + hardened container + allowlist proxy** (bollard/gVisor) ← **NEXT, but BLOCKED on a containerizer** (no docker/podman on Windows or WSL as of s33). Install Docker Desktop (elevated `winget install Docker.DockerDesktop`) OR docker.io in WSL2 (`sudo apt install docker.io`) to unblock. The online plane; the trifecta's exfil leg lives here, so its security layer comes last.
- **CaMeL-lite sink-policy primitive** ✅ **DONE (s34, ADR-044, co-designed with the user).** `crates/ferric-research/src/sink.rs`: `TaintSet` (substring taint over digest summary+quotes) + `SinkPolicy::decide(permission, tainted)` keyed off `PermissionLevel`, all 3 modes (`Deny`/`RequireApproval`/`Warn`, caller picks). 8 tests incl. the end-to-end gate shape. **Pure primitive — NOT wired into dispatch yet.**
- **inc 5 (remaining) — wire the sink policy into `registry.execute` + Loop research-phase wiring** ← run when the research→loop integration lands: populate a `TaintSet` from digests entering context; call `SinkPolicy::decide` beside the existing `check(permission, path)` at the dispatch chokepoint; route fetched content through the quarantine before the planner acts (a sprint-loops change).
- **PR open+merge as the STANDARD final loop phase** — promote sprint-loops `06-loop-phase.md` step #6 from optional to standard (small, clear; matches one-PR-per-sprint).
- **A testing system** — make the Loop's Test phase a real system (define first: containerized test runs? golden artifacts? coverage gates?).
- **Animus Launch (crate in Animus_Ferric)** — the interactive bootstrapper, once loop-hardening has momentum.
- A live small-model run measuring Ornstein summarization *quality* (safety is already structural).

## Earlier backlog (still open)
- Multi-file `apply_patch` (ADR-039 follow-on); GPU/edge run (Jetson/Pi → maybe L6); harder bench L7+; audio on real non-TTS audio + video modality. (MCP-stdio is now covered by the ADR-011-revision section above, no longer a standalone item.)

## Sprint 35 (expert review + refactor) — DONE, ADR-045
Full-project audit (security/efficiency/product-completeness); findings in
`sprints/s35/sprint-research/research-report.md`. Four fixes shipped:
- **Read-side sensitive-file guard** (`ferric-guard`) — `.env`/SSH keys/cloud credentials denied
  on `Read`, closing a real secret-into-plaintext-trace gap; `.git` metadata reads stay allowed.
- **`ferric server` edge-tuning flags** — `--threads`/`--gpu-layers`/`--batch-size` for
  Jetson/RPi-class latency tuning (llama-server only).
- **`mistralrs` rev-pinned** (was `branch = "master"`, matches the `oovra` policy).
- **`reqwest` → `rustls-tls`** (was pulling native OpenSSL via `default-tls`, confirmed via
  `cargo tree -e features`; edge/ARM cross-compilation win).
Explicitly deferred (reasons in ADR-045, not silently dropped): CaMeL sink-policy wiring (no live
taint source yet), `ferric mcp` + the new chat mode (own dedicated sprint — see above), shell/exec
+ git tools, streaming, session resume, trace rotation. Panic-safety sub-audit came back **clean**
(no unwrap/expect/panic! found on adversarial model/backend/file-content paths).

## Sprint 36 (ferric mcp — the ADR-005 security call) — DONE, ADR-046
User-prioritized (2026-07-03) from the GLM-review "critical gaps" list: built `ferric mcp`
(mistral.rs in-process hang explicitly dropped, not re-chased). MCP-stdio server exposing exactly
ONE tool (`ferric_query`, `{prompt, files?}`) — never Ferric's individual builtins, never
workspace/backend/model as per-call params (all launch-time-fixed → the containment guarantee is
structural). Hand-rolled JSON-RPC (no `rmcp` dep). All 7 build tasks (T-3601–T-3607) shipped;
research + plan in `sprints/s36/`. See completed-tasks.md for the per-task commit hashes.

## Production-readiness roadmap (external plan doc, reviewed 2026-07-03)
An independent "Production Ready Action Plan" (docx, dated sprint 34) was reviewed. It aligns with
the project's safety-before-blast-radius philosophy; its concrete future-task ideas, captured here
(NOT yet scheduled — each is its own future sprint, most already tracked above in some form):
- **Streaming inference** — ✅ **DONE, sprint 37** (user-chosen 2026-07-03, framed as "a base
  architectural choice"). See the Sprint 37 section below — token-by-token from the HTTP valve
  through the provider to CLI (`ferric query --stream` this increment; MCP/mistral.rs streaming and
  a structured/programmatic streaming mode are follow-ons, ADR-047).
- **Session resume** — replay the existing JSONL trajectory (ADR-002) to reconstruct loop state;
  `--resume <path>` + `--save-interval`. The reader's unknown-event tolerance already gives forward
  compat.
- **Persistent config** — ✅ **DONE, sprint 38** (user-chosen 2026-07-04, paired with `Animus.md`).
  See the Sprint 38 section below. `ferric init-project` (a scaffolding wizard) remains a follow-on.
- **`shell_exec` tool** — Ring 2 (Medium+); workspace cwd, command timeout, stdout/stderr capture,
  output caps; **extend Ornstein to screen commands** for destructive/exfil/privesc patterns before
  exec. (Needs the real permission-model extension flagged in ADR-045, not a quick add.)
- **`git` tool** — curated subset (status/diff/add/commit/log/branch/checkout); Ring 1 read / Ring 2
  write; reject force-push/rebase/reset unless an expert-only Ring 3 (10B+); subprocess not a git
  lib (dep weight).
- **Dev engine (`ferric dev`)** — the ADR-011-reserved self-modification arc (doc estimates ~3
  sprints, matching ADR-011's "s4–s7"): separate stricter loop with a MIN tier floor, `cargo check`
  in an isolated target dir, a distinct trajectory prefix, harness-source-protection rules in the
  guard (block edits to `ferric-guard`/denylists/workspace containment behind an explicit escape
  hatch), and **self-mod-specific guards** (modification-loop, scope-creep, regression).
- **Deployment hardening** — `cargo bloat`/`cargo tree --duplicates` binary-size budget enforced in
  CI per target (linux x86_64/aarch64, macOS aarch64, windows x86_64); **`oovra` supply-chain risk**
  (pinned to a personal-repo git rev) → vendor or migrate to a published crate; release packaging
  (GitHub Actions artifacts, `curl|sh`, cargo-binstall/Homebrew/AUR); docs site + cold-start
  onboarding.
- **Divergences from the doc, deliberately (already decided here):** (1) the doc slots MCP into its
  Phase 2 (sprints 38–40) as a **separate `ferric-mcp` binary exposing tool rings as MCP tool
  groups** — we shipped it EARLY (s36) as an **in-process `ferric mcp` subcommand exposing ONE
  `ferric_query` tool**, precisely because exposing individual tools/rings over MCP would let a
  client bypass the agent loop + guards (ADR-046's security call). Keep this divergence. (2) The doc
  is dated s34 (says "44 ADRs"); we're at ADR-046 / s36. (3) "Single-developer bus factor" and
  "external contribution / blog citation" success metrics are noted but out of scope for the
  harness itself.

## Sprint 37 (streaming inference) — DONE, ADR-047
User-chosen (2026-07-03), framed as "a base architectural choice." Fills ADR-003's reserved
streaming extension point: `Provider::complete_streaming` (default impl = zero behavior change for
non-overriding providers; `OpenAiProvider` gets a real SSE implementation via `Response::chunk()`,
no new dependency), the `ConstrainedJsonScanner` (incremental `task_complete` summary extraction,
handling JSON escapes correctly across chunk boundaries), `RunArgs.stream_sink` threaded through
the loop (`None` = byte-identical to today), and `ferric query --stream`. All 6 build tasks
(T-3701–T-3706) shipped; research + plan + two critique rounds in `sprints/s37/`. MCP streaming,
mistral.rs streaming, seamless mid-stream retry, and a structured/programmatic streaming mode are
explicit follow-ons, not built here. See completed-tasks.md for the per-task commit hashes.

## Sprint 38 (persistent config + `Animus.md`) — DONE, ADR-048
User-chosen (2026-07-04): "persistent config and Animus.md (much like claude.md but for Animus)".
Layered `.ferric/config.toml` (project) + cross-platform user config, CLI flag > project > user >
hardcoded default, for `ferric query`/`ferric mcp`'s tunables (backend, model, params_b/quant/
family/ctx/temperature, max_ring, profile_dir, stream) — a bounded, named `Config` field list
(never a generic key-value map), so config can't touch security/guard/denylist policy (ADR-005).
A foreground plan-critic pass (8 concerns, `sprints/s38/sprint-plans/critique.md`) caught a real,
non-obvious bug before it shipped: the ADR-029 profile-lookup key `model_key` would have been
derived from raw CLI args instead of the post-merge, config-resolved values — a config-only-set
`model` would have silently skipped its profile lookup with no error or trace. Fixing it surfaced
a SECOND instance of the same masking-hazard class mid-build: `BackendOpts.backend` itself still
carried a leftover clap default (unlike its 8 sibling fields), which would have made a config-only
`backend` invisible too — fixed the same way. `Animus.md` (the user's own framing: "much like
CLAUDE.md but for Animus") is read (no parsing) and folded into the system prompt as a distinct
block — trusted context (the workspace owner's own words), not Ornstein-quarantined. All 7 build
tasks (T-3801–T-3807) shipped; research + plan + critique in `sprints/s38/`. See completed-tasks.md
for the per-task commit hashes.

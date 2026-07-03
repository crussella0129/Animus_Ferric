# Sprint 35 Research Report — Expert review + refactor: operational/competitive/safe/efficient

## Sprint goal (in my words)
A comprehensive audit sprint: determine what Animus_Ferric needs to become an **operational,
competitive, and safe product**, while staying as **efficient as possible for edge and personal-
compute deployment** (Jetson/RPi-class + desktop). Unlike prior sprints (one narrow capability
each), this one's deliverable is primarily a **prioritized gap analysis**, with a small, contained
**refactor** executed directly against the highest-leverage, lowest-risk findings.

**Process note:** three background Explore agents (security, efficiency, product-completeness)
were launched for this audit but were stopped by the user before completing; per instruction they
were not relaunched. This report is instead built from (a) my own direct, code-verified audit
(file:line cited below), (b) an external review from GLM-5-turbo the user supplied, corrected
against the actual codebase, and (c) two live architectural decisions the user made mid-sprint.

## Decisions Reviewed
This sprint touches nearly every prior ADR by nature of being a full-project audit. Load-bearing
ones for the findings below: **ADR-004** (dependency allowlist), **ADR-005** (security
boundaries — `.ferric/` write-denied, host pinned to 127.0.0.1), **ADR-008** (deterministic
output), **ADR-010** (constraint XOR tools), **ADR-011** (command structure — see the revision
below), **ADR-012** (MCP-stdio, deferred since s1 — see the revision below), **ADR-040–044**
(Ornstein: quarantine, retrievers, orchestrator, sink policy — the sink policy's dispatch-wiring
gap is confirmed concretely below).

## Two decisions already made this session (context, not new research)
1. **Animus_Ferric is GGUF-only, permanently** (2026-06-29) — cross-format model loading is
   out of scope forever; pushed to the separate future **Animus Beast-Zoo** repo. Recorded in
   memory + `agent-tasks/agent-tasks.md`; not re-litigated here.
2. **ADR-011 revision decided: Ferric gets both `ferric mcp` (activating ADR-012) and a genuine
   raw chat mode** (2026-06-29), motivated by the upcoming **Animus IDE** organ needing one-off
   conversational change requests. This is a real, deliberate reversal of ADR-011's "no
   REPL/chat" clause, based on the user's hands-on experience since writing it. **Both pieces are
   large enough to deserve their own dedicated sprint(s)**, not a quiet tack-on here — the chat
   mode specifically touches the "harness never allows raw unconstrained chat" security thesis
   and needs its own ADR spelling out the security boundary. Recorded in memory +
   `agent-tasks/agent-tasks.md`.

## Findings — verified directly against the code (file:line cited)

### Security
- **No read-side sensitivity gate — MEDIUM.** `crates/ferric-guard/src/checker.rs:42-47`:
  `check(PermissionLevel::Read, _)` unconditionally returns `Allow`; only `Write`/`Execute` consult
  the denylist (`checker.rs:50-74`). Combined with `crates/ferric-tools/src/registry.rs:9-10`'s
  comment "the trace always gets the full output" (ADR-002, untruncated), a workspace containing a
  secret (`.env`, an SSH key, a cloud credential file) can be `read_file`'d by the agent and the
  full contents will be persisted **in plaintext** in the JSONL trace. This is a real gap: the
  write-side denylist (`DENIED_WRITE_SEGMENTS`/`DENIED_WRITE_FILES`) has no read-side counterpart.
- **CaMeL-lite sink policy confirmed NOT wired — HIGH (known, now confirmed by exact location).**
  `crates/ferric-tools/src/registry.rs:126-160` (`Registry::execute`, the dispatch chokepoint) only
  calls `ferric_guard::check(spec.permission, &resolved)` — zero reference to
  `ferric_research::sink::SinkPolicy`/`TaintSet`. The primitive built in sprint 34 has no live
  effect until this chokepoint calls it. (Already tracked in the backlog; this confirms the exact
  wiring point.)
- **`mistralrs` dependency floats on `branch = "master"`, not even rev-pinned — MEDIUM (supply
  chain / reproducibility).** `Cargo.toml:35`: `mistralrs = { git = "...", branch = "master" }`.
  Unlike `oovra` (`Cargo.toml:40`, rev-pinned with an explicit "immutable until a deliberate,
  reviewed bump" policy), a `cargo update` can silently pull a different mistralrs commit at any
  time, changing build behavior with no diff to review. Feature-gated off by default (`Cargo.toml`
  comment, line 22-24) so this only bites `--features backend-mistralrs` builds, but it's worse
  hygiene than the project's own stated policy for git deps.
- **Panic-safety audit came back clean — POSITIVE finding, not a gap.** Grepped
  `\.unwrap\(\)|\.expect\(|panic!` across `ferric-loop/src`, `ferric-provider/src`, and
  `ferric-tools/src/builtin` (the three surfaces that touch model output, backend responses, and
  file content respectively). Every hit outside test modules was verified structurally safe: the
  two live-path unwraps in `crates/ferric-loop/src/grammar.rs:29-30` are on regex capture groups
  that are non-optional in the pattern, so they're guaranteed `Some` whenever the outer match
  succeeds; the `openai.rs:358-411` hits I initially flagged are all inside `#[cfg(test)]` (verified
  by reading the surrounding module). Zero `unwrap`/`expect`/`panic!` in any builtin tool. No
  DoS-via-panic vector found on adversarial model/backend/file input in the paths checked.
- **Workspace boundary logic holds up well — POSITIVE.** `crates/ferric-guard/src/workspace.rs`:
  canonicalized `Component`-sequence containment, symlink-resolved before the check, prefix-
  collision-safe (tested: `project-evil` vs `project`, `workspace.rs:142-149`), `..`-traversal
  rejected via `lexical_normalize` (`workspace.rs:66-81`). No issues found.

### Efficiency / edge deployment
- **No CPU/GPU resource tuning in `ferric server` — HIGH (directly blocks the stated edge goal).**
  `crates/ferric-cli/src/server.rs:98-131` (`command()`, the llama-server argv builder) emits only
  `-m`, `--mmproj`, `-c`, `--host`, `--port`. **No `-t`/`--threads`, no `-ngl`/`--n-gpu-layers`, no
  batch-size flag** — `ferric server up` always defers to llama-server's own defaults. There is no
  way today to tell Ferric "use N threads" or "offload M layers to GPU," which matters directly for
  a Jetson/RPi target where the right thread count and GPU-layer split are the primary levers for
  usable latency.
- **No streaming — HIGH (UX, not edge-specific, but compounds latency on slow edge hardware).**
  `crates/ferric-provider/src/types.rs:98-107`: `Completion` is a single flat struct
  (`message, input_tokens, output_tokens, truncated`) with no chunk/stream variant; `Provider::
  complete()` returns one `Result<Completion, _>` per call. On an edge device where a turn may
  take many seconds, the user/caller sees nothing until the full completion lands.
- **`mistralrs`/`tokio` correctly default-OFF — POSITIVE, already well-managed.** `Cargo.toml:22-
  24` comment confirms the default workspace build and the aarch64 CI gate never compile the heavy
  ML dep; it only pulls in under `--features backend-mistralrs`. The default `ferric-cli` binary is
  lean by design — this part of the edge story is already right.
- **`reqwest` TLS feature not overridden — LOW/MEDIUM, unverified.** `Cargo.toml:36`: `reqwest = {
  features = ["json"] }` doesn't disable default features, which (unless reqwest's own default
  changed) typically pulls a TLS backend requiring native OpenSSL bindings on non-Windows targets.
  For cross-compiling to ARM edge boards, `rustls-tls` (pure Rust, no native OpenSSL/cross-toolchain
  dependency) is usually the leaner choice. Flagged as needing verification (check reqwest's actual
  resolved default features) before treating as confirmed.
- **Trace files have no rotation/size cap — LOW/MEDIUM for edge storage.**
  `crates/ferric-trace/src/lib.rs:1-5`: "Writers flush per event" — every event is written
  immediately, and nothing in the trace module handles rotation, size limits, or retention. Each
  session gets its own JSONL file (not unbounded within one file across sessions), but a single
  long-running or tool-output-heavy session, or many accumulated session files with no cleanup
  utility, could matter on flash-constrained edge storage (RPi SD card, Jetson eMMC) over time.
- **Prompt-budget math is sound and edge-tested — POSITIVE.**
  `crates/ferric-core/src/scale.rs:186-188,297-304`: `prompt_budget_tokens = min(ctx*7/10,
  tier_cap)`, tested down to a 4096-token context (`prompt_budget_respects_small_context`). Scales
  cleanly to small edge-model context windows; no hardcoded large-model assumption found.

### Product completeness / competitive gaps
- **CLI surface confirmed: exactly 5 subcommands, no more.** `crates/ferric-cli/src/main.rs:30-48`:
  `query`, `bench`, `toolbench`, `server {up,down,status,doctor}`, `trace cat`. No `mcp`, no `dev`,
  no chat/REPL — matches `main.rs:1-5`'s own doc comment ("ADR-011 — no chat catch-all... `ferric
  dev` — reserved... s4–s7"), still unbuilt as of s34.
- **No shell/exec or git tools — confirmed structurally.** The 12 registered builtins (`ferric-
  tools/src/builtin/mod.rs`) are all filesystem-scoped: read/write/edit/list/move/mkdir/search/
  delete/find/copy/multi_edit/apply_patch. Zero process-execution or git-operation tools. For a
  *coding* agent this is a real functional ceiling — it cannot run tests, run a build, or inspect
  git history/diffs as part of a task.
- **`docs/` has exactly 5 files, no getting-started doc.** `multimodal.md`, `testbench.md`,
  `llama-cpp.md`, `ornstein.md`, `beast-zoo-spec.md`. No single onboarding doc, no CONTRIBUTING, no
  API reference — a new user has to piece together usage from the README + these five topic docs.
- **No session resume / error recovery.** Confirmed structurally: `query` is one-shot (no `--
  continue`/session-ID flag in `main.rs`'s `QueryArgs` surface); if the loop stops mid-task (any
  `StopReason`), there is no mechanism to resume from where it left off.
- **GPU support is entirely llama-server's responsibility, not Ferric's** — consistent with the
  "no resource-tuning flags" finding above; Ferric has no GPU-aware logic of its own.

### From the external (GLM-5-turbo) review — corrected and cross-referenced
The user supplied an external review. Verified against the code above; three corrections made:
1. **"Over-engineered for current scope"** — reframed as **safety/reliability infrastructure has
   outpaced functional breadth**, not wasted effort. The guard family (repetition/no-progress/
   failure, s22/27/28) and the tool-ring system each closed a specific, empirically observed
   failure mode from the bench ladder — not speculative gold-plating. The actionable takeaway is
   closing the functional gap (shell/git tools, streaming) *using* that infrastructure, not
   distrusting it.
2. **"No tests for live backends" as a weakness** — this is the *correct* trade-off, not a gap: CI
   cannot depend on live GGUF models (no guaranteed availability, nondeterministic, GPU-dependent).
   The project's actual pattern (`#[ignore]`-gated tests + manual `bench --backend openai` runs,
   producing real results across sprints 20-26) is the standard answer to this constraint.
   Confirmed by reading `ferric-provider/src/mock.rs`'s central role in the test suite — deliberate,
   not accidental.
3. **Crate count / minor factual slips** — the review says "9 crates" then lists 10; corrected to
   10 (workspace `Cargo.toml:3-13`). The "burnout-driven quality degradation" claim is speculation
   with no supporting evidence (test/ADR discipline has been consistent through s34) — dropped.

The review's core gap list (no streaming, no shell/git tools, no MCP, no persistent config, no
session resume, mistral.rs in-process backend broken under constraint, thin docs) is **independently
confirmed** by my own direct code reading above.

## Risks / unknowns / dependencies
- The `reqwest` TLS-feature finding needs verification (check `cargo tree -e features` or similar)
  before being treated as confirmed rather than probable.
- The scope here is large — more findings than one sprint can responsibly fix. The plan phase must
  prioritize; see recommendation below.

## Recommended approach
Given the volume of findings, split into three horizons rather than one undifferentiated task list:

1. **This sprint's refactor (small, contained, zero new architecture, all directly serve
   safe/efficient goals):**
   - Wire the CaMeL sink policy into `Registry::execute` (closes the confirmed dangling primitive).
   - Add a read-side sensitive-file check (extend the guard: deny/warn on reading well-known secret
     patterns, at minimum matching the existing write-side denylist).
   - Add CPU-thread / GPU-layer / batch-size passthrough flags to `ferric server` (direct edge win).
   - Pin `mistralrs` to a specific rev instead of `branch = "master"` (trivial, real reproducibility
     fix).
   - Verify and likely switch `reqwest` to `rustls-tls` (edge cross-compilation + supply chain).
2. **New ADR(s) recording this sprint's findings** — an audit ADR (gap analysis, honestly scoped)
   distinct from the two live decisions already recorded (GGUF-only; ADR-011 revision).
3. **Explicitly deferred to their own future sprints** (too large/security-sensitive for a
   refactor sprint): `ferric mcp` (ADR-012 activation, needs the still-pending "ADR-005 security
   call"); the new raw chat mode (needs its own dedicated ADR on the security boundary); a
   shell/exec tool (needs a real permission-model extension, likely a new `PermissionLevel` or a
   heavily sandboxed variant — not a quick add); git tools; streaming; session resume; trace
   rotation.

### Alternative considered — try to fix everything this sprint
Rejected: the project's own proven pattern (Ornstein's five-increment build, the three-guard
family built one-at-a-time) is narrow, well-tested increments, not broad simultaneous changes. A
"fix everything" sprint would produce untested, unreviewed sprawl — exactly what a *review and
refactor* sprint should avoid modeling.

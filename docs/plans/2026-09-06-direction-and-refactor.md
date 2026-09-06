# Ferric — Direction & Refactor Plan

**Date:** 2026-09-06
**Trigger:** First human use test (`Animus_Ferric_Human_Use_Test_1`) failed; red CI; repo bloat concern; Rust reviewer flagged enormous files needing module decomposition.

This is a working plan, not a sprint record. Fold the sequenced work into the
normal `dev` → PR → `main` cadence (one PR per sprint) as you see fit.

---

## 1. What the human test actually showed

The run did **not** panic from overwriting its own directory. The real chain was
three compounding faults hidden behind one generic error:

1. **Wrong working directory.** `ferric` Work mode defaults its workspace to the
   current directory (`crates/ferric-cli/src/human.rs:11`, `:89`). Launched with
   `cargo r` from the repo root, "folder work here" targeted the repo itself and
   created `test1/` inside the source tree. It also scanned the repo-local
   `models/` folder for GGUFs.
2. **No visible progress.** Streaming is implemented and on by default
   (`complete_streaming`, ADR-047; `config.rs:251`) and Work mode calls it
   (`human.rs:530`), but the human surface only renders `StreamDelta::Text`
   (`human.rs:523`). In the constrained/grammar protocol the model emits opaque
   action JSON — **no Text deltas** — so the screen stays blank until a tool call
   lands. On a 27B-on-CPU model that is minutes of apparent death.
3. **Unusable model choice.** The picker offered a 15.3 GiB 27B and ran it on CPU
   with "resource fit is not measured." Nothing steered the user to a model that
   could actually respond interactively.

The user Ctrl+C'd (reasonably). That set the cancel flag; the provider returned
`Backend("Interrupted")` (`openai.rs:28`); the loop retried, gave up
(`run.rs:377`), and reported `provider_error` — masking all three causes.

**Separately:** the `ferric` on PATH is a *stale build* lacking the `run`
subcommand (hence `unrecognized subcommand 'run'`, tip suggests `cron`), while
the freshly built `target\debug\ferric.exe` has it (`main.rs:79`). The welcome
text tells users to type `ferric run "..."` (`human.rs:50`). Deployment has
drifted.

---

## 2. Bloat — diagnosed and (mostly) already reclaimed

**Done this session (working tree, uncommitted):**

- `git lfs prune` + `git gc --prune=now` → **`.git` 17 GB → 7.9 MB.** The 17 GB
  was ~15 GB of dead Git-LFS objects (nothing is currently LFS-tracked; no
  `.gitattributes`) plus ~1 GB of un-gc'd loose objects. **No history rewrite was
  needed** — the actual packed history is 25 MB.

**You should run (20 GB, your model data, your destination):**

```bash
# models/ is already gitignored (line 33) — this just moves it out of the repo
# working tree into the shared Animus model store the rest of the suite uses.
mv /c/Users/charl/Animus_Ferric/models/* /c/Users/charl/Animus/Models/
```

**The real "bloat" is the code, not the repo.** Tracked content is tiny (~5 MB
source + ~15 MB docs). But:

- **128K LOC across 16 crates.** ~35.6K LOC of that is server-lifecycle /
  Tailscale / process-ownership plumbing — **nearly equal to the entire agent
  core** (loop + tools + provider + prompt = 37.5K).
- **The crate graph itself is clean and well-layered** (see §4). The monolith is
  *inside one crate*, `ferric-cli` (68K LOC), in flat multi-thousand-line files.
- `server.rs` is **18,257 lines** (≈6.5K production + ≈11.7K inline tests), 88
  types, 413 functions, one file.

Untracked working-dir cruft to sweep (none is committed): `e2e_log_*.txt`,
`e2e_test_log*.txt`, `e2e_workspace*/`, `fleet_*.jsonl/md`, `report_*.jsonl/md`,
`ring0/1.*`, `toolbench_*.log`, `ferric-mock.txt`, `job.log`, `target/`.

---

## 3. Red CI — fixed this session

Single flaky test: `query::tests::powershell_quote_round_trips_argv`
(`query.rs`). It validates PowerShell quoting against the *real* parser by
spawning `powershell.exe` with a 10 s budget. On the post-merge Windows runner
PowerShell cold-start exceeded 10 s (`script_entered=false`) — environmental, not
a quoting regression (green on the PR run, red on main).

**Fix applied (uncommitted):** raised the process budget 10 s → 60 s with a
comment explaining cold-start headroom; the job's own `timeout-minutes` still
bounds a true hang. Verified locally: passes in 503 ms.

This is symptomatic of a broader pattern — **unit tests that spawn real
processes / real PowerShell / real servers.** Workstream C moves those to a
dedicated integration lane so the default `cargo test` gate is fast and
deterministic.

---

## 4. The architecture: what's actually wrong, and the target

The **crate dependency graph is fine** — it's a clean DAG:

```
leaves:   core, process, guard, cron, vcs, skills, animus-launch
mid:      prompt→core   provider→core   trace→core   tools→core,guard   icm→guard
          research→core,provider        bench→core,process,trace
core:     ferric-loop → core, provider, guard, tools, trace, vcs      ← the agent harness
binary:   ferric-cli  → ALL 15 crates                                 ← the god-crate
```

`ferric-loop` (+ tools + provider) **is already a coherent, standalone agent
harness** (~37K LOC). The problem is `ferric-cli`: a 68K-LOC binary that mixes
five unrelated concerns in giant flat files:

| Concern | Files | ~LOC |
|---|---|---|
| **Inference-server lifecycle + Tailscale serving** | `server.rs`, `server_registration.rs`, `server_process.rs`, `server_resolution.rs`, `tailscale_serve.rs`, `tailscale_localapi.rs`, `test_process_containment*.rs`, (`api.rs`) | **~35K** |
| Human surface | `human.rs`, `chat.rs`, `startup/*`, `config.rs`, `main.rs`, `logging.rs` | ~7K |
| Expert commands | `autonomy_cmd.rs`, `bench_cmd.rs`, `toolbench_cmd.rs`, `mcp.rs`, `icm.rs`, `cron.rs`, `dream_cmd.rs`, `revert_cmd.rs`, `launch.rs`, `skills_cmd.rs` | ~9K |
| Trace tooling | `trace_verify.rs`, `trace_cmd.rs` | ~1.5K |
| One-shot query | `query.rs` | ~3K |

### Target shape

1. **Extract the server/serving cluster into its own crate: `ferric-serve`**
   (~35K LOC leaves `ferric-cli`). It depends on `ferric-process`, `ferric-core`,
   `ferric-guard`; `ferric-cli` depends on it. This makes the agent harness
   (`ferric-loop` + `ferric-tools` + `ferric-provider`) genuinely usable without
   dragging in Tailscale/HTTP-serving/process-adoption machinery — the "lean
   harness" the reviewer and the use test are pointing at.
2. **`ferric-cli` becomes a thin dispatcher**: arg parsing + one small module per
   command that calls into library crates. Target: no file over ~800 LOC.
3. **The agent harness can ship/standalone** behind the CLI without the serving
   layer — the split is what makes a "100× smaller harness" reachable without
   throwing away the safety work; the infra just stops being *in the middle of
   the harness*.

---

## 5. Module decomposition (the reviewer's point)

Break the giant files into modules/submodules **by the responsibility clusters
already visible in the code**. `server.rs` is the flagship; it decomposes almost
mechanically along its own type clusters:

```
crates/ferric-serve/src/
  lib.rs                 # re-exports; the ServerCommand dispatch entry
  cli.rs                 # Engine, ServerCommand, *Args, ServerConfig, ServerRunfile   (server.rs:54–404)
  runtime.rs             # SpawnedChild/ProcessRuntime/ListenerInspector/HealthProbe/
                         #   LifecycleClock traits + Native impls + reap/shutdown       (server.rs:527–770)
  managed.rs             # ManagedServer/-State/-Discovery, registration revision,
                         #   DiscoveryFingerprint, LifecycleDiscovery/Observation       (server.rs:1006–1232)
  doctor.rs              # PendingManagedDiscovery, DoctorProbeEffects                   (server.rs:2090–2129)
  publication.rs         # Tailscale publish: PublicationStage/Completion/Compensation,
                         #   ProxyCleanup, ProxyReconcile                                (server.rs:2259–2585)
  launch.rs              # LaunchOrchestration{Error,Success}, RenderedPublicationReport (server.rs:3415–3510)
  adoption.rs            # AdoptionAlias/Rollback/Report/Disposition                     (server.rs:3936–3997)
  registration.rs        # (from server_registration.rs, split by its own type groups)
  process.rs             # (from server_process.rs)
  resolution.rs          # (from server_resolution.rs)
  tailscale/serve.rs     # (from tailscale_serve.rs)
  tailscale/localapi.rs  # (from tailscale_localapi.rs)
  containment.rs         # (from test_process_containment.rs)
```

**Tests:** ~11.7K of `server.rs`'s lines are one inline `mod tests`. Move
per-cluster unit tests next to each new module, and move real-process /
real-PowerShell / lifecycle-fixture tests into `crates/ferric-serve/tests/`
(integration lane), not `src/`. Do the same for the other in-`src/` test files:
`live_budget_tests.rs`, `human_journey_tests.rs`, `test_process_containment_tests.rs`.

**Other decomposition targets (same principle, smaller):** `autonomy_cmd.rs`
(4059), `query.rs` (3135), `trace_structure.rs` (loop, 3251), `controlled_file.rs`
(tools, 3171).

**Rule for every split:** pure code motion + `mod`/`pub(crate)` visibility only —
no behavior change in a decomposition PR, so `cargo test` is the proof. Keep
decomposition PRs mechanical and reviewable; never mix a refactor with a fix.

---

## 6. Last-mile usability (what the human test needs to succeed)

**U1 — Workspace guard + honest default.** Refuse folder-work in a VCS root or in
Ferric's own source tree unless explicitly opted in (`--workspace <dir>` or a
confirmation). Show the resolved absolute workspace path before enabling edits.
*Root cause of the `test1`-in-repo incident.*

**U2 — Deploy story.** One canonical installed binary. Document/support
`cargo install --path crates/ferric-cli` (or a release artifact) so nobody runs
`cargo r` from the repo. Fix the stale-PATH drift and align the welcome text
(`human.rs:50`) with the actual command surface. Fix the duplicate `[[bin]]`
targets both pointing at `src/main.rs` (`ferric` and `ferric-lifecycle-test`),
which emits the multi-target build warning.

**U3 — Progress indicator that works in constrained mode.** The human surface
must show life even when there are no `StreamDelta::Text` deltas: a spinner /
elapsed timer / token-arrival heartbeat, and tool-call previews from the
`ConstrainedJsonScanner` signals. Never let a Work session look frozen.

**U4 — Resource-fit-aware model selection.** Estimate fit (model size vs. RAM/VRAM)
in the picker; default to / recommend the smallest capable model; warn or
de-rank models unlikely to run interactively. Replace "resource fit is not
measured" with an actual signal.

**U5 — Clearer interruption reporting.** Distinguish user-interrupt from a real
backend error in the exit message (Ctrl+C → "You stopped the run", not
`provider_error`).

---

## 7. Suggested sequencing (one PR per sprint, `dev` → `main`)

Ordered by value-to-effort; each line is a candidate sprint/PR.

1. **Land the reclaim + CI fix + models move + cruft sweep.** (This session's
   `query.rs` change is ready; add `.gitignore` sweep for the root cruble, verify
   CI green on `dev`.) *Small, unblocks everything.*
2. **U1 + U5 (workspace guard + interrupt reporting).** Directly fixes the test's
   #1 failure; small, high-signal.
3. **U3 (progress indicator).** Fixes the "nothing is happening" experience.
4. **U2 + U4 (deploy + model fit).** Makes a clean first-run reproducible.
5. **Extract `ferric-serve` crate** (code motion; behavior-preserving). Biggest
   structural win; shrinks `ferric-cli` by a third.
6. **Decompose `server.rs` into the §5 module tree** (mechanical, PR-per-cluster
   if needed) and move real-process tests to the integration lane.
7. **Decompose the remaining giants** (`autonomy_cmd`, `query`, `trace_structure`,
   `controlled_file`) and audit for any dead expert commands to retire.

---

## 8. Principles going forward

- **Least code.** The codebase is already criticized for infra-ahead-of-breadth;
  prefer deleting/decomposing over adding. A fix and a refactor never share a PR.
- **No real processes in the default test lane.** Real PowerShell / real server /
  lifecycle-fixture tests live in `tests/` integration lanes with generous
  budgets, gated so `cargo test` stays fast and deterministic.
- **The harness is the product; serving is a peripheral.** Keep `ferric-loop` +
  `ferric-tools` + `ferric-provider` clean and independently usable. Everything
  Tailscale/HTTP/process-adoption is a satellite, not a core dependency.
- **Behavior-preserving refactors are proven by the existing test suite** — that
  is exactly what the large test corpus is *for*; lean on it during the split.

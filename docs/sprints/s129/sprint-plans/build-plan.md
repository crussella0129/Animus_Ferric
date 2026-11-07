# Sprint 129 Build Plan — Minimality refactor

**Goal (owner, 2026-11-07):** run a redundancy-analysis swarm over the whole
codebase, plan a refactor to absolute minimality that keeps the code
professional and readable, and then carry it out.

**Rules for every step:**

- Behaviour is preserved unless a row says otherwise. Error and diagnostic
  strings that tests or users rely on stay byte-identical.
- Security-critical ordering stays exactly as it is: cap-std no-follow
  checks, guard order, gate order, signal-before-cleanup.
- Each step passes `cargo fmt`, workspace `clippy -D warnings`
  (default features, `--no-default-features`, and the Windows target), and
  the workspace tests. The test count is compared against the sprint-start
  baseline: the only failures allowed are the 19 sandbox-only lifecycle
  failures (T-12810), and tests may be removed only where a row here says so.
- Retirements (§E) are **not** carried out without the owner's approval.

Status: **done** = landed in this sprint; **open** = carried as a task in
`docs/work/tasks.md` (T-129xx).

## A. Mechanical removals and crate folds

| ID | Change | Source findings | Est. |
|---|---|---|---|
| A-1 | Remove the iron re-export shims from ferric-loop and ferric-provider and import from `ferric_iron`. Delete `tests/iron_reexports.rs` and the loop exports that have no consumer. | R-ARCH-4, R-LOOP-9, R-MODEL-2 | −150 |
| A-2 | Delete ferric-provider's sprint-0 `mock_loop_skeleton.rs` and its four dev-dependencies. | R-ARCH-3, R-MODEL-5 | −183 |
| A-3 | Manifest hygiene: remove unused and doubled dependencies, use `workspace = true` pins, make provider's tokio follow `backend-openai`, fix stale comments. | R-ARCH-9, R-TOOLS-15 | −15 |
| A-4 | Delete the CI `backend-check` job; the lifecycle-fixture clippy step already compiles the same code. | R-ARCH-10, R-MODEL-14 | −11 |
| A-5 | Delete dead public items. These are listed in full in the source findings; they include `managed_models_dir`, `Gate::allowed_hosts`, `run_spec`, `Invocation::mock`, `TierSource::from_label`, `WebRetriever::with_airlock`, `SsePayload` and `MIN_TAINT_SEGMENT_CHARS`. | R-ARCH-11, R-TOOLS-11/12/14, R-BENCH-5, R-CLI-11, R-MODEL-7, R-ENG-11, R-LOOP-15 | −400 |
| A-6 | Delete the inert `RunPolicy` fields. INT-0011 AC-7 and T-11406 already authorize this. | R-MODEL-6 | −35 |
| A-7 | Delete unreachable re-checks in `replay()` and the no-op branch in `TraceStructure`. | R-LOOP-8 | −25 |
| A-8 | Delete test-only legacy APIs that live in production files. | R-SRV-11 | −230 |
| A-9 | Fold the six single-consumer crates: ferric-vcs into `ferric_loop::vcs`; ferric-cron, ferric-skills, ferric-prompt, ferric-icm and animus-launch into ferric-cli. 20 → 14 crates. | R-ARCH-1, R-MODEL-9 | −220 |
| A-10 | Remove stale Docker scaffolding (an unreferenced script, compose tombstones, a wrong comment and a redundant feature flag), and drop the redundant mock mode of `tools/e2e_test.ps1`. | R-ARCH-7, R-ARCH-13 | −65 |

## B. Defects found by the swarm

| ID | Fix |
|---|---|
| D-1 | Retry a transient `STATUS_PARTIAL_COPY` command-line read on Windows; every other status still fails closed. |
| D-2 | `git_read` refuses `branch` with arguments (creating a branch is a write). |
| D-3 | `ferric server up` and `server doctor` locate the engine the same way `ferric run` does: PATH, then the engine `ferric setup` installed. |
| D-4 | The provider reports an in-band SSE `error` event as a provider error. |
| D-5 | Repair the broken line continuations in model-facing and error text. |
| D-6 | Replace the concrete tailnet address in `docs/work/tasks.md` with the documentation value. |
| D-7 | `ferric explain` lists models from the directory `ferric run` resolves. |
| D-8 | Align live and resumed runs on nudging after a truncated turn, with a regression test. |
| D-9 | `icm init` uses the same symlink-safe, create-new scaffolder as `ferric launch` (one helper, R-ARCH-1). |
| D-10 | `MIN_TAINT_SEGMENT_CHARS` is deleted (A-5): it was documented but never applied, and the guard's actual rule is the one its tests pin. |

## C. Consolidations in production code (behaviour-preserving)

| ID | Change | Source | Est. |
|---|---|---|---|
| C-1 | ferric-loop: one `record_event` (write, then project) for the 52 pairs; one guard-verdict helper for the 6 ladders; one backoff loop; `StopReason::parse` replaces the re-typed string sets; `#[derive(Default)]` on the four guards. | R-LOOP-2/3/7/10 | −290 |
| C-2 | ferric-loop: the legacy dispatch step shares `edit_preview`, `permission_event` and the approval gate with controlled dispatch. | R-LOOP-4 | −85 |
| C-3 | ferric-loop: `ReplayedState` embeds `RecoveryCheckpointV1` instead of copying it, and the dead `turns` field goes. | R-LOOP-5 | −70 |
| C-4 | ferric-tools: one navigation core for `find_files` and `search_files`; one registry guard loop and sink block; shared argument parsing (`builtin/args.rs`); pure edit helpers shared by run and prepare; one git runner for `git_read` and `git_write`; `Workspace::admit` and `Workspace::relative` in ferric-guard. | R-TOOLS-1/2/3/6/8/9 | −780 |
| C-5 | ferric-tools: run_check and shell_exec use `ferric_process::run_bounded`; delete `blocking.rs`. | R-TOOLS-5 | −200 |
| C-6 | One no-follow file-identity module in ferric-tools, used by tools, startup storage, model discovery and query. | R-TOOLS-4, R-ARCH-6, R-ENG-8 | −200 |
| C-7 | Shared small helpers: `is_sha256_hex` and `user_dir` in ferric-core; one `sha256_bytes`; one `now_unix_ms`. | R-ARCH-12/14, R-TOOLS-13, R-BENCH-7 | −80 |
| C-8 | ferric-cli: one `RunFlags` clap group and resolver for query, chat, mcp, api and icm; `RunConfig::run_args` replaces `LoopSetup` and the seven `RunArgs` literals; `ResolvedBackend` serves every surface. | R-CLI-1/2/3 | −600 |
| C-9 | ferric-cli: a single `Result<ExitCode, String>` boundary per command; shared resume, `Animus.md`, attachment and research folding; `create_trace_sink` for chat and icm. | R-CLI-8/9/10 | −205 |
| C-10 | `trace verify` and the autonomy trace analysis reuse `TraceStructure` instead of re-checking its rules. | R-CLI-7, R-BENCH-3 | −250 |
| C-11 | ferric-bench: one ladder spawn path; `MechanismCounts`; shared JSONL and ledger helpers; one prefix rule; no second trace verification; enums and checks reused from the bench crate. | R-BENCH-2/4/10/11/12/13 | −430 |
| C-12 | Pure OpenAI wire helpers move once into ferric-iron (`wire.rs`) and are used by the provider and the valve; one request path inside the provider; small valve duplicates removed. | R-MODEL-1/4/10 | −290 |
| C-13 | ferric-core keeps only shared vocabulary: media classification moves to ferric-cli, and the hand-rolled base64 becomes the `base64` crate. | R-MODEL-8 | −40 |
| C-14 | Server lifecycle: one conditional isolate-and-compare helper; one registration store seam; one discovery observation model; one rendered report type; one down path; shared capture helpers; `command(&ServerUpArgs)`; drop the wide envelope. | R-SRV-4/5/6/8/9/12/15/16 | −875 |
| C-15 | Engine and process: libc and windows-sys instead of hand-written FFI; one version probe and engine locator in ferric-setup; `MemoryProbe` removed; small human.rs duplicates. | R-ENG-4/7/9/13 | −245 |

## D. Test consolidation (test-only)

| ID | Change | Source | Est. |
|---|---|---|---|
| T-1 | ferric-loop tests share `args()`, `read_records()`, `git_workspace()` and one scripted runner. In-module fixtures are shared, copied helpers move to `tests/common`, and same-level duplicates are deleted. | R-LOOP-1/11/12/13/14 | −1,110 |
| T-2 | ferric-cli integration tests share `tests/support.rs` (isolated command, bounded output, one fake OpenAI server) and trace helpers. Same-level duplicate tests are deleted. | R-CLI-4/5/6 | −670 |
| T-3 | Server tests: one persistence double, one down double, shared fixtures, one native-test module; no tests that call other tests; no copied renderer used as an oracle. | R-SRV-3/10/14 | −800 |
| T-4 | Process tests: one exact-process observer in `ferric_process`, test containment moved into ferric-process, one native-smoke harness, and one fake engine and GGUF writer. | R-ENG-1/2/3/5/6/10, R-ARCH-8 | −1,280 |
| T-5 | ferric-tools, bench, trace and core redundant tests and copied fixtures. | R-TOOLS-10, R-BENCH-6/14, R-MODEL-11/12/13/15 | −870 |

## E. Retirements: owner decision required (not done without approval)

| ID | Retire | Why it is redundant | Est. | Cost to users |
|---|---|---|---|---|
| E-1 | Tailscale Serve publication (`ferric server up --tailscale`) | `ferric tunnel` (sprint 127) serves the same goal without a coordination server, which the owner called a centralization risk | −11,500 | Windows loses remote access until `ferric tunnel` supports Windows |
| E-2 | Schema-1 runfiles, boolean-only Tailscale records and `ferric server adopt` | Nothing has written them since 2026-08-30 | −2,600 | A pre-August engine must be stopped by hand |
| E-3 | Paired Legacy-vs-Evidence autonomy A/B (`bench autonomy --paired …`) | INT-0010 calls the Evidence controller it measures abandoned; the runner is maintenance-only | −2,500 | The paired qualification mode disappears |
| E-4 | The Legacy `Tool::run` dispatch path | It duplicates prepare/commit | −450 | Legacy-policy runs move to the prepare/commit path |
| E-5 | The backend-free ferric-cli build | `backend-openai` has been the default since sprint 120, and every shipped build includes it | −250 | `--no-default-features` no longer builds an offline-only binary; `--mock` remains |
| E-6 | `HarnessPolicy::EvidencePlanner` | An abandoned arm that every layer rejects | −120 | The flag value becomes invalid instead of "not implemented" |
| E-7 | Replay compatibility for traces from before sprint 113 | Format boundaries from August | −150 | Resuming a session interrupted before 2026-08-26 is refused |
| E-8 | The `TailnetFsRetriever` remote research plane | It is never constructed, and SSH is blocked on its only target | −335 | None (dead code), but a backlog item names it |
| E-9 | `tools/install.sh` and `tools/install.ps1` | Their "no backend" mode is broken, and the root installers and one documented `cargo install` cover them | −86 | Two developer wrappers go |

## Order of work

1. B (D-1) and A, which are mechanical and verifiable by the compiler.
2. The rest of B, the defects.
3. C, area by area. Each area is one commit, verified on its own.
4. D, area by area.
5. Records, LOC measurement and the PR. Present §E to the owner.

## Outcome

| Group | Result | Commits |
|---|---|---|
| A. Mechanical removals and crate folds | Done: A-1 to A-10, 20 → 14 crates | `301608d`, `06a33be`, `2a77cac` |
| B. Defects | Done: D-1 to D-9 with regression tests; D-10 by deletion; D-11 (CI zombie, below) | `ff403d3`, `06a33be`, `f3cbf89`, `bb04540`, `31c57b5` |
| C. Consolidations | Done per area: C-1 to C-6 and C-8 to C-15, with the items in T-12908 deferred for recorded reasons; C-7 deferred | `bb04540`, `491b7df`, `68ca8ed`, `80de775`, `eb3afe9`, `63bc20b`, `31c57b5` |
| D. Test consolidation | Done per area (T-1 to T-5), each removal named with its covering test | same commits |
| E. Retirements | Not done; presented to the owner (T-12906) | — |

- **Size:** 140,082 → 131,421 lines of Rust (−8,661, −6.2%), 20 → 14
  crates, 1,478 → 1,414 tests.
- **Why the reduction is smaller than the analysts' total estimate:** the
  retirements in §E hold about 18K of the removable lines, and several
  consolidations were deferred because they would have changed behaviour.

**D-11.** CI on Linux failed from the crate fold onwards:

- The folded launch tests left a zombie (git's detached maintenance), and
  that zombie was adopted by the subreaper test process.
- An unprivileged caller cannot list a zombie's descriptors, so the
  engine-ownership check refused every later engine.
- Fixed by treating a zombie as holding no sockets.
- The same investigation found that the check refuses for any non-root
  Linux user outside a PID namespace (T-12907).

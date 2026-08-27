# Sprint 81 — Full-Codebase Verification

**Date:** 2026-07-24
**Scope:** `Animus_Ferric` @ `dc15da3` (main, post-PR #41) — 14 crates, ~29,700 lines
of Rust — plus `Animus_Dark_Matter` @ `685b799` (main, post-PR #1).
**Method:** static read-through with file:line citations, cross-crate usage
analysis, dependency reachability, git archaeology on suspected regressions.

---

## 0. Verification status — read this first

| Check | Result |
|---|---|
| Static audit, 14 Ferric crates | **complete** |
| Dark Matter spec verifier (`scripts/verify-spec.sh`) | **PASS 61 / FAIL 0** |
| `cargo test --workspace` | **BLOCKED — not run** |
| `cargo clippy --all-targets` | **BLOCKED — not run** |
| `cargo fmt --check` | **BLOCKED — not run** |

The dynamic checks are blocked by an environment condition, not by the code:
the `C:` volume is at **100% (241 MB free)**. A `cargo test --workspace` was
started, ran ~45 minutes, and wedged: 6 live `cargo`/`rustc` processes with
**zero artifacts written to `target/debug/deps` in the preceding 5 minutes**
while free space continued to fall. Debug test binaries for this workspace are
~100 MB each and there are ~20 of them; they cannot link.

**The workspace has 484 tests. None of them were executed this sprint.** Every
finding below is from reading the source, and each is cited so it can be
checked independently. Findings A1–A7 are defects I can demonstrate by
inspection; none of them are "a test failed", because no test ran.

**Remedy (needs your approval — these are your files, not mine):**
`target/` measures **49 GB** and holds three profiles — `debug/`, `release/`,
and `aarch64-unknown-linux-gnu/`. All are gitignored and fully regenerable, and
`target/` alone is ~5% of the volume. Clearing the cross-compile profile first
is the least disruptive move:

```bash
cargo clean --target aarch64-unknown-linux-gnu
```

If that is not enough, `cargo clean` reclaims the full 49 GB at the cost of one
cold rebuild. (`models/` is a further 4.4 GB and is deliberately kept.)

---

## 1. What is critical and effective

The load-bearing core is genuinely good. This is not filler — these files are
the ones that would survive any rewrite.

| File | Why it earns its place |
|---|---|
| `crates/ferric-core/src/scale.rs` | The founding idea, cleanly expressed: `ModelProfile → RunPolicy` is pure and total, `measured_level` overrides the size prior in both directions, and the tier table is pinned by a snapshot test so every calibration change is a reviewed diff. |
| `crates/ferric-guard/src/workspace.rs` | Best code in the tree. Containment decided on canonicalized `Component` sequences, never string prefixes; symlinks resolved before the check; the `project` vs `project-evil` prefix-collision bug is covered by a named test (`workspace.rs:149`). |
| `crates/ferric-tools/src/registry.rs` | The single chokepoint. Guard runs *before* the handler, denial means the handler never executes, and the ring-aware cap trims from the outer ring so the core vocabulary can never be silently dropped (`registry.rs:112`). |
| `crates/ferric-loop/src/projector.rs` | The sprint-44 event-sourced refactor. One state machine reconstructs the context window from trace events, killing the run/replay dual-maintenance problem. (Also where defect A1 was introduced — see below.) |
| `crates/ferric-trace/src/sink.rs` | Append-only JSONL, one flush per event, so a crashed session still leaves a complete trace. Correct priority: durability over throughput. |
| `crates/ferric-loop/{repetition,progress,failure}.rs` | Three guards keyed off different axes (name+args / name / result) composing by threshold 2 < 3 < 5. A clean, honest design — and honestly scoped: they bound wasted compute, they do not lift a capability ceiling. |
| `crates/ferric-cron/src/lib.rs`, `crates/animus-launch/src/lib.rs` | Closed command sets, deterministic, no LLM in the loop. Cron can only ever run `dream` or `query`, never arbitrary shell (`lib.rs:335`). |

---

## 2. Defects

Ranked by consequence.

### A1 — Tool-output truncation is dead; the model sees full, untruncated output

`Registry::execute` computes `ToolOutput.for_model` — the 4,000-char truncated
view that ADR-002 says goes into the prompt while `full` goes to the trace
(`registry.rs:15`, `registry.rs:234`). The loop receives it and **discards it**:

- `run.rs:756` — the field is named `_for_model` (underscore = deliberately unread)
- `run.rs:775` — `_for_model: output.for_model`
- `run.rs:458` — the `ToolResult` event is written with `result_text.full`
- `projector.rs:114` — the projector feeds that event's `output` straight back into `messages`

So the full output re-enters the context window every turn. `for_model` has
exactly one consumer in the entire workspace, and it is a skeleton test:
`crates/ferric-provider/tests/mock_loop_skeleton.rs:135`.

**When it broke:** `git log -S` puts the introduction of `_for_model` at
`705c55f` — "Sprint 44: Event-Sourced Projector Refactor". Before that, the
sprint-1 loop used `output.for_model`. The projector reconstructs context from
the trace, and the trace correctly stores `full`; the truncated view had no
place to live in the new design and was dropped rather than carried.

**Consequence:** a single `search_files` or `shell_exec` over a large tree can
blow the prompt budget in one turn, on every tier. This most plausibly explains
context pressure on small models that the compaction path then has to clean up.

### A2 — The taint set tracks the wrong value, so the sink policy cannot fire

`crates/ferric-cli/src/query.rs:926-929`:

```rust
for d in multi.digests {
    taint_set.taint_str(&d.source);   // ← marks the PROVENANCE LABEL
    cx.push_str(&d.summary);          // ← injects the UNTRUSTED CONTENT
```

`ResearchDigest.source` is documented at `ferric-research/src/lib.rs:51` as
*"Provenance: where the content came from. Stamped by the harness."* — it is a
relative file path, and it is trusted. The untrusted content is `summary` and
`claims[].quote`.

So the CaMeL-lite policy (ADR-044) is inverted against its own threat model:

- **False negative (the one that matters):** an injection lives in `summary`.
  If the model copies injected text into a `write_file`, `args_tainted`
  (`guard/sink.rs:100`) does not match — the tainted string in the set is a file
  path, not the injected text. The gate opens.
- **False positive:** the model legitimately writing to the file it just
  researched trips `Deny`, because the path *is* in the taint set.

This is a one-line fix (`taint_str(&d.summary)` plus each quote), but the
current code means ADR-044's guarantee does not exist in the live path.

Related: `d.claims` are never used by the only live consumer — the structured
claim/quote pairs the quarantine produces are built and thrown away.

### A3 — `ferric-vcs` destroys the user's git index, once per turn

`crates/ferric-vcs/src/lib.rs:36-52` runs `git add -A`, writes a tree, then
`git reset`. `run.rs:256-257` calls this **every single turn**.

`git reset` (mixed) unstages everything. If you have a carefully staged index
and run Ferric in that repo, the index is wiped on turn 1 and every turn after.

The code knows it is unfinished. Line 50, verbatim:

```rust
// Wait, `add -A` pollutes the staging area. We can use `git read-tree HEAD` to reset it.
// Even better: just leave it or `git reset` (mixed).
```

That is unresolved think-aloud shipped to `main`. `git read-tree HEAD` — the
option the comment names first — is the correct fix and does not touch the
working tree.

Also here: `revert` runs `git clean -fd` (`lib.rs:71`), deleting untracked files
with no confirmation.

### A4 — `manage_task` can panic the whole harness, and the model can call it

`crates/ferric-tools/src/builtin/manage_task.rs` holds 9 `.unwrap()`s on lock
acquisition (`status.write().unwrap()`, `child.lock().unwrap()` — lines 45, 49,
79, 81, 127, 132, 151, 157, 171) plus 3 more in `task_registry.rs` (44, 49, 53).

One panicking task thread poisons the mutex, and then **every subsequent
`manage_task` call aborts the process**. In a codebase whose stated thesis is
that model-driven paths never panic, this is the one model-invokable tool that
can kill the harness.

Two more panic paths in the same file: `tokio::runtime::Handle::current()`
(line 163) panics without an ambient runtime, and `block_in_place` (line 162)
panics on a current-thread runtime. `ferric-loop` is explicitly
executor-agnostic and drives mocks on `futures_executor`.

`send_input` also has a race: stdin is `take()`n under one lock (line 157-159),
written outside it, then restored under a *different* lock acquisition
(line 171). Two concurrent calls interleave, and a panic in between loses the
pipe permanently.

This file has **zero tests**.

### A5 — The web sandbox's airlock is off by default

`WebRetriever::new()` (`ferric-research/src/web.rs:17-25`) sets
`proxy_url: None` and `enforce_runsc: false`; `run_in_sandbox`
(`sandbox.rs:47`) then attaches `--network bridge`. The result is a container
with `--cap-drop=ALL` and `no-new-privileges` but **unrestricted network
egress** — the allowlist proxy and the gVisor runtime are both config knobs that
nothing in the tree ever sets.

Severity is currently bounded by D2 below: `WebRetriever` is unreachable from
the binary. But the default is the wrong way round — a security-critical
constructor should require you to opt *out* of the airlock, not into it.

### A6 — `fetch_reference` returns nothing for short-token queries

`tokenize` (`builtin/fetch_reference.rs:201-207`) drops tokens of 2 characters
or fewer. A query of `"Go"`, `"AI"`, `"C"`, or `"k8"` therefore produces an
empty term list, every chunk scores 0, the `> 0` filter at line 86 rejects
everything, and the tool reports "No reference chunk matched" over a vault that
may be full of matches.

### A7 — `RequireApproval` silently degrades to `Deny`, next to a shipped approver

`registry.rs:207-213` turns `SinkDecision::RequireApproval` into a denial,
commenting "require-approval not wired". But ADR-070 (sprint 79) shipped
`EditApprover` (`ferric-loop/src/run.rs:53`) — a human-in-the-loop gate at the
dispatch site, which is precisely the missing mechanism. Two human-approval
systems were built four sprints apart and never introduced to each other.

---

## 3. Vestigial — safe to remove

| # | Item | Evidence |
|---|---|---|
| B1 | `ferric_core::Protocol` enum + `RunPolicy.protocol` field | `FencedCode`/`EditFormat` are **never constructed or matched anywhere** in the workspace; `policy_for` hardcodes `ConstrainedJson` (`scale.rs:206`) and `query.rs:235` comments that "`select_protocol` ignores the policy". Fully superseded by `ActionProtocol` (ADR-015/022). **Caveat: it is serialized into profiles and traces — removal is a schema change, not a pure deletion.** |
| B2 | `Registry::with_truncation_limit` | `registry.rs:87` — only caller is `new()`. Becomes live again if A1 is fixed. |
| B3 | 6 unused dependencies | `thiserror` in ferric-bench / ferric-tools / ferric-trace; `ferric-core` in ferric-guard; `ferric-guard` in ferric-research; `tokio` in ferric-vcs (used only by `#[tokio::test]` — belongs in `[dev-dependencies]`). |
| B4 | `LoopState.registry_tools` | `run.rs:91` carries an explicit `#[allow(dead_code)]`. |
| B5 | `DispatchText._for_model` | `run.rs:756`. Deleting it is only correct *after* A1 is fixed — until then it is the evidence. |
| B6 | `SandboxConfig::default()` | `sandbox.rs:19` — never called; `WebRetriever::new` duplicates the same literal inline. |
| B7 | `test-sweep-prompt.txt` (repo root) | Byte-identical to `workspace/test-sweep-prompt.txt` (verified by `diff`). |
| B8 | `_parse_error` | `run.rs:281` — action parse failures are computed and dropped on the floor. Nothing reaches the trace, so a grammar failure is indistinguishable from an empty completion in post-hoc analysis. Either surface it as a `Note` or delete the binding. |

---

## 4. Refactor candidates

**C1 — `run_with_provider` takes 18 positional parameters** (`query.rs:789-808`)
and its entire body re-packs them into `RunArgs`, a struct that already exists.
Six call sites (query, chat ×2, icm, mcp, api) each pass 18 positional
arguments; four `#[allow(clippy::too_many_arguments)]` suppressions exist to
keep it quiet. Having the callers build `RunArgs` directly is mechanical, and it
removes the exact shape where argument-order bugs live. **Highest
value-to-risk ratio of anything in this report.**

**C2 — The `post_turn` hook block is copy-pasted four times** in `run.rs`
(lines 311-322, 483-493, 502-512, 541-551) — identical bodies. Extract
`fn fire_post_turn(&mut self) -> Result<(), FerricError>`.

**C3 — `ferric-vcs` is fake-async.** `snapshot` and `revert` are `async fn`
with no `.await` in the body; they call blocking `std::process::Command`
(`lib.rs:76-88`). Under tokio this blocks a reactor thread every turn. Either
make them honestly synchronous or use `tokio::process`.

**C4 — `task_registry` is a process-global static with no removal path**
(`task_registry.rs:24`). Tasks and their `Child` handles accumulate for the life
of the process; `list_tasks` returns everything ever spawned; completed children
are never reaped. Needs a `remove_task` and a reap pass.

**C5 — `manage_task`'s status-string match is duplicated** (lines 56-63 and
88-95, character-identical). Extract a helper — and give the file tests.

**C6 — `prompts/protocol-unified-grammar.md` is misnamed.** It is the `TextXml`
atom that teaches `<tool_call>` XML; the name survives from before the
`UnifiedGrammar → ConstrainedJson` rename. `ferric-prompt/src/lib.rs:59` carries
a comment apologising for it. Rename to `protocol-text-xml.md`.

**C7 — `ferric-cli` is 9,689 lines across 19 flat modules** — a third of the
whole codebase in one crate, with `mcp.rs` (1,172), `query.rs` (1,154) and
`toolbench_cmd.rs` (960) as the bulk. The shared spine (`run_with_provider`,
`backend::create_provider`) is correctly factored; it is the command modules
that want subdirectories.

**C8 — Test-runner scripts are scattered across three locations in two shell
dialects:** `e2e_test.ps1` / `run-tool-sweep.ps1` / `run_benchmarks.ps1` at the
root, `tools/run-e2e.sh` + `tools/run-coverage.sh`, and
`workspace/run-e2e-sweep.sh`. Consolidate under `tools/`.

---

## 5. Built, tested, and unreachable

Not vestigial — these are deliberate forward investments — but no code path in
the binary reaches them, and that should be stated plainly rather than
discovered later.

- **D1 — `TailnetFsRetriever`** (ADR-042, ~half of `retriever.rs`): zero
  references outside its own crate. Live SSH E2E was deferred in sprint 32 and
  still is.
- **D2 — `WebRetriever` + `sandbox.rs`** (ADR-045, 201 lines): zero references
  outside the crate. `query.rs:915-920` constructs **only** `LocalFsRetriever`.
  There is no CLI flag that can produce a web plane.
- **D3 — The sink policy is inert everywhere except `ferric query --research`.**
  `chat.rs:302/324/562`, `icm.rs:352`, `mcp.rs:448`, `api.rs:265` and
  `trace_verify.rs:163` all pass `TaintSet::new()` — an empty set short-circuits
  `args_tainted` to `false` (`sink.rs:101`), so the policy never fires there.
  Correct for now (no taint source), worth knowing.

---

## 6. Dark Matter, and the fetch-vs-fold seam

**State of the repo:** `Animus_Dark_Matter` is **specification-only** — 17
tracked files, no code. `SPEC.md` (42 KB), `INTEGRATION.md`, `decisions.md`, a
`template/` skeleton, and `scripts/verify-spec.sh`. The MCP knowledge server and
the `mirror` ingestion pipeline are both contract-only (SPEC §6.5, §10).

**Its verifier is green:** `scripts/verify-spec.sh` → **PASS 61, FAIL 0**.

**The Ferric side works and is measured.** ADR-071 reports a stage-1 prompt
dropping 136,162 → 3,355 chars (97.5%) on a 133 KB vault. `ComposeMode` is
flag-gated so `compose_stage` is byte-for-byte unchanged
(`ferric-icm/src/lib.rs:357-386`), which is the right way to carry an A/B.

**But the two repos currently document different tools under one name.**

| | Dark Matter `INTEGRATION.md` / SPEC §6.2 | Ferric `builtin/fetch_reference.rs` |
|---|---|---|
| Required arg | `target` (**the only one**) | `query` (`:40`) |
| `target` | corpus bound to the active stage | **absent entirely** |
| `query` | optional | required |
| `section`, `k=4` | ✅ | ✅ (`:37`, `:19`) |
| Return | `{chunks:[{uri,text,score}], truncated}` | flat markdown, `### ref://…` headers (`:99-107`) |
| `score` | returned | computed (`:210`) then dropped |
| `truncated` | returned | **not signalled** — `k`-capping at `:90` is silent |

Three consequences worth deciding on:

1. **INV-3 is realized by a different mechanism, and it is narrower.** DM §6.3
   requires the MCP server be *"the only process with filesystem access to
   `03_reference/`… there is no direct-read code path"*, making the gate
   missing-by-construction. Ferric's tool **is** a direct read
   (`fetch_reference.rs:128`), substituting ADR-065 stage containment. That
   substitution is defensible and arguably stronger — the guard applies to
   *every* tool, not just this one — but it can only express "the stage's own
   `references/`". DM's INV-3 is `r ∈ bindings(active())` over multiple named
   corpora, and Ferric has no way to bind 2 of 5 shared corpora. **Either DM
   narrows INV-3 to one-corpus-per-stage, or Ferric grows `target`.**

2. **The zero-marginal-token claim does not hold on the Ferric path.** DM §6.1
   rests it on `ttlMs`/`cacheScope` making repeat reads free. Ferric re-reads and
   re-chunks the entire vault on every call (`collect_chunks`, `:77`) and
   re-emits full chunk text into context each time. The 97.5% number is a
   *first-fetch* measurement, and it is real; the *repeat*-fetch property is not
   implemented.

3. **DM's verifier cannot see any of this.** `test_ferric_citations_resolve`
   (`verify-spec.sh:223-228`) only checks that two Ferric *file paths exist*. It
   would pass if `fetch_reference.rs` were empty. If the seam is to stay honest
   across two repos, that check should assert the actual `input_schema` in
   `crates/ferric-tools/src/builtin/fetch_reference.rs` against the descriptor in
   `INTEGRATION.md`.

---

## 7. Operational

- **F1 — `C:` is full** (241 MB free / 931 GB). Blocks all cargo work.
  `Animus_Ferric/target/` is **49 GB** of it and `models/` a further 4.4 GB;
  everything else in the repo totals under 10 MB. See §0.
- **F2 — `refs/ferric/*` snapshot refs accumulate with no GC.** Eight already
  exist in this repo from prior runs. They are correctly namespaced (they do not
  pollute branches — verified: zero snapshot commits reachable from `main`), but
  nothing ever prunes them, and each pins a full tree object.
- **F3 — A `llama-server.exe` has been running since 09:56** and is unrelated to
  this session.
- **F4 — `.gitignore` is in good shape.** The root-level `e2e_log_*.txt`,
  `ring0.jsonl`, `report_*.md`, `fleet_*.jsonl`, `job.log`, `toolbench_*.log`
  clutter is all untracked and correctly ignored. Only B7 is actually committed.

---

## 8. Suggested order of work

1. **F1** — free disk, then actually run `cargo test` / `clippy` / `fmt`. Nothing
   below is verified until this happens.
2. **A1** — restore `for_model`. One-line-ish, largest live impact.
3. **A3** — `git read-tree HEAD` instead of `git reset`. Prevents data loss.
4. **A2** — taint `summary` + quotes. Makes ADR-044 real.
5. **A4** — de-panic `manage_task`; add tests.
6. **C1** — `run_with_provider(RunArgs)`. Mechanical, removes 4 clippy allows.
7. **B3, B7, C6** — trivial cleanups, no behavioural risk.
8. **§6** — decide the `target` question with Dark Matter before its s2 build
   hardens the other side of the contract.

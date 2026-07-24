# Full-Codebase Verification — sprint 82

**Date:** 2026-07-24
**Scope:** `Animus_Ferric` @ `dc15da3` (main, post-PR #41) — 14 crates, 29,664
lines of Rust — plus `Animus_Dark_Matter` @ `685b799` (main, post-PR #1).
**Supersedes:** the sprint-81 static audit, which was written blind. This pass
re-derives every finding against a green toolchain and, where a defect could be
made to fail a test, *made it fail one*.

---

## 0. Verification status

Sprint 81 produced this same map by reading source, but could not run a single
check: the `C:` volume was at 100% and `cargo test` wedged mid-link. The user
cleared `target/` (49 GB) before this sprint. Every blocked check now ran.

| Check | s81 | s82 |
|---|---|---|
| Cold `cargo build --workspace --all-targets` | blocked | **clean, 0 warnings, 31s** |
| `cargo test --workspace` | blocked | **463 passed / 0 failed**, 52 suites, exit 0 |
| `cargo clippy --workspace --all-targets` | blocked | **0 warnings**, exit 0 |
| `cargo fmt --all --check` | blocked | **clean**, exit 0 |
| DM `scripts/verify-spec.sh` | PASS 61 / FAIL 0 | **PASS 61 / FAIL 0** |

484 `#[test]` attributes exist; 463 execute, 2 are `#[ignore]`d, the remainder
are `cfg`-gated. **The suite is genuinely green — and it is green *through* four
of the defects below.** That gap is the most useful thing this sprint learned.

Findings are tagged **PROVEN** (a test was written that fails on current `main`),
**CONFIRMED** (verified at the cited line), or **CORRECTED** (s81 was wrong).
The probe tests were run, recorded, and removed; the tree is unmodified.

---

## 1. Critical and effective — the load-bearing core

This is not filler. These files would survive any rewrite, and the green suite
genuinely covers them.

| File | Why it earns its place |
|---|---|
| `crates/ferric-guard/src/workspace.rs` | Best code in the tree. Containment decided on canonicalized `Component` sequences, never string prefixes; symlinks resolved before the check. The `project` vs `project-evil` prefix-collision bug has a named regression test. 10 tests. |
| `crates/ferric-core/src/scale.rs` | The founding idea, cleanly expressed: `ModelProfile → RunPolicy` is pure and total, `measured_level` overrides the size prior in both directions, and the tier table is pinned by a snapshot test so every calibration change is a reviewed diff. |
| `crates/ferric-tools/src/registry.rs` | The single chokepoint. Guard runs *before* the handler; denial means the handler never executes. Ring-aware capping trims from the outer ring so the core vocabulary can never be silently dropped. 79 tests. |
| `crates/ferric-loop/src/projector.rs` | The sprint-44 event-sourced refactor: one state machine reconstructs the context window from trace events, killing run/replay dual maintenance. (Also where A1 was introduced.) |
| `crates/ferric-trace/src/sink.rs` | Append-only JSONL, one flush per event — a crashed session still leaves a complete trace. Durability over throughput is the right call. |
| `crates/ferric-loop/{repetition,progress,failure}.rs` | Three guards keyed off different axes (name+args / name / result), composing by threshold 2 < 3 < 5. Honestly scoped: they bound wasted compute, they do not lift a capability ceiling. |
| `crates/ferric-cron/src/lib.rs`, `crates/animus-launch/src/lib.rs` | Closed command sets, deterministic, no LLM in the loop. Cron can only ever run `dream` or `query`, never arbitrary shell. |

---

## 2. Defects

### A1 — PROVEN — tool-output truncation is dead; the model sees full output

`Registry::execute` computes `ToolOutput.for_model`, the 4,000-char view ADR-002
says goes into the prompt (`registry.rs:234`). The loop discards it:

- `run.rs:756` — the field is `_for_model` (underscore = deliberately unread)
- `run.rs:458` — the `ToolResult` event is written with `result_text.full`
- `projector.rs:114` — the projector feeds that event's `output` back into `messages`

`for_model` has exactly one consumer in the workspace, and it is a skeleton test
(`ferric-provider/tests/mock_loop_skeleton.rs:135`).

**Proof.** A probe drove the real loop: write a 20,000-char file, read it back,
inspect the next provider request.

```
SPRINT82_PROBE longest_user_message_chars=20028
```

20,028 chars reached the context window where ADR-002 promises ≤ 4,000 — a **5×
budget overrun on a single tool call**, on every tier.

**Why the green suite misses it.** `ferric-loop/tests/truncation_tests.rs` sounds
like coverage and is not: it tests *token-budget-truncated model completions*
(a cut-off action), an unrelated mechanism. The only test of output truncation is
`registry.rs:423`, a unit test on the Registry — which passes, because the
Registry end of the contract is correct. **No test crosses the crate boundary
where the value is dropped.** Introduced in `705c55f` (sprint 44); the truncated
view had no place in the event-sourced design and was dropped rather than carried.

### A2 — CONFIRMED — the taint set tracks the wrong value, so the sink policy cannot fire

`crates/ferric-cli/src/query.rs:927-928`:

```rust
taint_set.taint_str(&d.source);   // ← the PROVENANCE LABEL (trusted)
cx.push_str(&d.summary);          // ← the UNTRUSTED CONTENT
```

`ResearchDigest.source` is documented at `ferric-research/src/lib.rs:51` as
harness-stamped provenance — a relative path. The untrusted content is `summary`
and `claims[].quote`. ADR-044's CaMeL-lite policy is inverted against its own
threat model:

- **False negative (the one that matters):** an injection lives in `summary`. If
  the model copies it into a `write_file`, `args_tainted` does not match — the
  tainted string is a file path, not the injected text. The gate opens.
- **False positive:** legitimately writing to the researched file trips `Deny`.

One-line fix (`taint_str(&d.summary)` plus each quote). Related: `d.claims` are
never read by the only live consumer — the structured claim/quote pairs the
quarantine exists to produce are built and thrown away.

### A3 — PROVEN — `ferric-vcs` destroys the user's git index, once per turn

`crates/ferric-vcs/src/lib.rs:36-52` runs `git add -A`, writes a tree, then
`git reset`. `run.rs:256` calls this every turn.

The code knows it is unfinished. Lines 49-51, verbatim:

```rust
// Wait, `add -A` pollutes the staging area. We can use `git read-tree HEAD` to reset it.
// Even better: just leave it or `git reset` (mixed).
```

That is unresolved think-aloud shipped to `main`.

**Proof.** Stage one of two changed files, take one snapshot:

```
SPRINT82_PROBE staged_before="staged.txt" staged_after=""
```

The staged set is destroyed on turn 1 and every turn after.

**CORRECTION — s81's recommended fix is also wrong.** s81 endorsed the comment's
`git read-tree HEAD`. Measured in a scratch repo, that discards the staged set
*identically* (`BEFORE staged: staged.txt` → `AFTER: ''`) — both commands reset
the index to HEAD. The correct fix is to never touch the real index: run the
snapshot under a temporary `GIT_INDEX_FILE`. Verified —

```
BEFORE staged: 'staged.txt '
AFTER temp-index snapshot staged: 'staged.txt '
tree contents: base.txt staged.txt untracked.txt
```

— the user's index survives *and* the snapshot still captures untracked files.

Also here: `revert` runs `git clean -fd` (`lib.rs:71`), deleting untracked files
with no confirmation.

### A4 — CONFIRMED (with corrections) — `manage_task` can panic the harness

`crates/ferric-tools/src/builtin/manage_task.rs` holds **9** `.unwrap()`s on lock
acquisition (lines 45, 49, 79, 81, 127, 132, 151, 157, 171), plus **3** in
`crates/ferric-tools/src/builtin/task_registry.rs` (s81 cited this file one
directory too high). One panicking task thread poisons the mutex, and every
subsequent `manage_task` call then aborts the process. In a codebase whose thesis
is that model-driven paths never panic, this is the one model-invokable tool that
can kill the harness.

Two more panic paths: `Handle::current()` (line 163) panics without an ambient
runtime and `block_in_place` (line 162) panics on a current-thread runtime —
while `ferric-loop` is explicitly executor-agnostic and drives mocks on
`futures_executor`.

`send_input` also races: stdin is `take()`n under one lock (157-159), written
outside it, then restored under a *different* acquisition (171).

**CORRECTION — s81 claimed "this file has zero tests".** It has zero *inline*
tests, but `crates/ferric-tools/tests/background_tasks.rs` covers
spawn → list → status → kill. The accurate statement: the happy path is covered;
**every panic path, the `send_input` race, and all concurrent access are not.**

### A5 — CONFIRMED — the web sandbox's airlock is off by default

`WebRetriever::new()` (`web.rs:17-25`) sets `proxy_url: None` and
`enforce_runsc: false`; `run_in_sandbox` (`sandbox.rs:47`) then attaches
`--network bridge`. The result has `--cap-drop=ALL` and `no-new-privileges` but
**unrestricted network egress** — the allowlist proxy and gVisor runtime are both
knobs nothing in the tree ever sets. Severity is currently bounded by D2 (the
type is unreachable), but the default is the wrong way round: a security-critical
constructor should require opting *out* of the airlock.

### A6 — PROVEN — `fetch_reference` returns nothing for short-token queries

`tokenize` (`fetch_reference.rs:201-207`) drops tokens of ≤ 2 chars, so `"Go"`,
`"AI"`, `"C"`, `"k8"` produce an empty term list, every chunk scores 0, and the
`> 0` filter rejects everything.

**Proof.** A vault whose heading *and* body are entirely about Go:

```
SPRINT82_PROBE fetch_reference(query="Go")
  -> "No reference chunk matched query \"Go\" (searched 1 chunk(s) under
      `references/`). Try broader keywords."
```

The tool saw the chunk and rejected it. Note the filter is `t.len() > 2` on a
`&str` — byte length, so it also mis-handles short multibyte terms.

### A7 — CONFIRMED — `RequireApproval` silently degrades to `Deny`

`registry.rs:207-213` turns `SinkDecision::RequireApproval` into a denial,
commenting "require-approval not wired". But ADR-070 (sprint 79) shipped
`EditApprover` — a human-in-the-loop gate at the dispatch site, which is exactly
the missing mechanism. Two human-approval systems built four sprints apart, never
introduced to each other.

---

## 3. Vestigial — safe to remove

| # | Item | Status |
|---|---|---|
| B1 | `ferric_core::Protocol` enum + `RunPolicy.protocol` | **CONFIRMED.** `FencedCode`/`EditFormat` appear only in their own declaration and one doc comment — never constructed, never matched. Superseded by `ActionProtocol`. **Caveat: serialized into profiles and traces, so removal is a schema change, not a pure deletion.** |
| B2 | `Registry::with_truncation_limit` | **CONFIRMED** — sole caller is `new()`. Becomes live again if A1 is fixed. |
| B3 | 6 unused dependencies | **PROVEN.** `thiserror` in ferric-bench/-tools/-trace; `ferric-core` in ferric-guard; `ferric-guard` in ferric-research all removed, and `tokio` moved to `[dev-dependencies]` in ferric-vcs → `cargo check --workspace --all-targets` **exit 0**. Reverted after measuring. |
| B4 | `LoopState.registry_tools` | **CONFIRMED** — carries `#[allow(dead_code)]`; `self.registry_tools` has zero reads. (The identically-named *local* at `run.rs:593` is live — the dead thing is the struct field.) |
| B5 | `DispatchText._for_model` | **CONFIRMED** — correct to delete only *after* A1 is fixed; until then it is the evidence. |
| B6 | `SandboxConfig::default()` | **CONFIRMED** — never called; `WebRetriever::new` duplicates the literal inline. Note it is a `pub` impl, so removal is a (pre-1.0) API change. |
| B7 | `test-sweep-prompt.txt` at repo root | **CONFIRMED** — byte-identical to `workspace/test-sweep-prompt.txt`; both tracked. |
| B8 | `_parse_error` (`run.rs:281`) | **CONFIRMED** — action parse failures computed and dropped. Nothing reaches the trace, so a grammar failure is indistinguishable from an empty completion in post-hoc analysis. Surface it as a `Note` or delete the binding. |

---

## 4. Refactor candidates

**C1 — `run_with_provider` takes 18 positional parameters** (`query.rs:789-807`)
and its body immediately re-packs them into `RunArgs`, a struct that already
exists. Five `#[allow(clippy::too_many_arguments)]` suppressions exist to keep it
quiet. Having callers build `RunArgs` directly is mechanical and removes the
exact shape where argument-order bugs live. **Highest value-to-risk ratio in this
report.**

**C2 — the `post_turn` hook block is copy-pasted four times** in `run.rs`
(311-322, 483-493, 502-512, 541-551), identical bodies. Extract one method.

**C3 — `ferric-vcs` is fake-async.** `snapshot` and `revert` are `async fn` with
**zero `await` in either body**; they call blocking `std::process::Command`.
Under tokio this blocks a reactor thread every turn. Make them honestly sync, or
use `tokio::process`.

**C4 — `task_registry` is a process-global static with no removal path.** Tasks
and their `Child` handles accumulate for the life of the process; `list_tasks`
returns everything ever spawned; completed children are never reaped.

**C5 — `manage_task`'s status-string match is duplicated** (lines 56-63 and
88-95, character-identical). Extract a helper — and give the file unit tests.

**C6 — `prompts/protocol-unified-grammar.md` is misnamed.** It is the `TextXml`
atom teaching `<tool_call>` XML; the name survives the
`UnifiedGrammar → ConstrainedJson` rename, and `ferric-prompt/src/lib.rs:59`
carries a comment apologising for it. Rename to `protocol-text-xml.md`.

**C7 — `ferric-cli` is 9,689 lines across 19 flat modules** — a third of the
codebase in one crate, led by `mcp.rs` (1,172), `query.rs` (1,154),
`toolbench_cmd.rs` (960). The shared spine is correctly factored; it is the
command modules that want subdirectories.

**C8 — test-runner scripts are scattered** across three locations in two shell
dialects: `e2e_test.ps1` / `run-tool-sweep.ps1` / `run_benchmarks.ps1` at the
root, `tools/run-e2e.sh` + `tools/run-coverage.sh`, and
`workspace/run-e2e-sweep.sh`. Consolidate under `tools/`.

---

## 5. Built, tested, and unreachable

Deliberate forward investments — not vestigial — but no path in the binary
reaches them, which should be stated plainly rather than discovered later.

- **D1 — `TailnetFsRetriever`** (ADR-042, ~half of `retriever.rs`): **CONFIRMED**
  zero references outside its own crate. Live SSH E2E deferred since sprint 32.
- **D2 — `WebRetriever` + `sandbox.rs`** (ADR-045, 201 lines): **CONFIRMED** zero
  references outside the crate. `query.rs:915` constructs only `LocalFsRetriever`;
  no CLI flag can produce a web plane.
- **D3 — the sink policy is inert everywhere except `ferric query --research`.**
  **CONFIRMED**: `api.rs:265`, `chat.rs:302/324/562`, `icm.rs:352`, `mcp.rs:448`,
  `trace_verify.rs:163` and even `query.rs:624/650/1059` all pass
  `TaintSet::new()`. An empty set short-circuits `args_tainted` to `false`, so the
  policy never fires there. Correct for now (no taint source) — worth knowing.
  Combined with A2, ADR-044's guarantee is currently unrealized *everywhere*.

---

## 6. Dark Matter, and the fetch-vs-fold seam

**State:** `Animus_Dark_Matter` @ `685b799` is **specification-only** — 17 tracked
files, no code. `SPEC.md` (42 KB), `INTEGRATION.md`, `decisions.md`, a `template/`
skeleton, `scripts/verify-spec.sh`. The MCP knowledge server and the `mirror`
ingestion pipeline are both contract-only (SPEC §6.5, §10). Its verifier is green:
**PASS 61, FAIL 0**.

**The Ferric side works and is measured.** ADR-071 reports stage-1's prompt
dropping 136,162 → 3,355 chars (97.5%) on a 133 KB vault. `ComposeMode` is
flag-gated so `compose_stage` is byte-for-byte unchanged — the right way to carry
an A/B.

**But the two repos document mutually-incompatible tools under one name.**

| | DM `INTEGRATION.md` / SPEC §6.2 | Ferric `builtin/fetch_reference.rs` |
|---|---|---|
| Required arg | `target` (the only one) | `query` (`:40`) |
| `target` | corpus bound to the active stage | **absent entirely** |
| `query` | optional | required |
| `section`, `k=4` | yes | yes (`:37`, `:19`) |
| Return | `{chunks:[{uri,text,score}], truncated}` | flat markdown, `### ref://…` headers |
| `score` | returned | computed (`:210`) then dropped |
| `truncated` | returned | **not signalled** — `k`-capping is silent |

**PROVEN, both halves.** A probe issued the exact call from `INTEGRATION.md`:

```
SPRINT82_PROBE dm_legal_call -> Err("missing required string argument: query")
SPRINT82_PROBE return_payload -> "### ref://runtime.md#0\n# tokio spawn\n\nUse tokio::spawn…"
```

A DM-schema-legal call is **hard-rejected** by Ferric, and the return is markdown
where DM specifies a JSON envelope. This is not a documentation drift; it is two
incompatible contracts sharing a name.

Three decisions worth making before DM's s2 build hardens the other side:

1. **INV-3 is realized by a different mechanism, and it is narrower.** DM §6.3
   requires the MCP server be the only process with filesystem access to
   `03_reference/` — the gate missing-by-construction. Ferric's tool **is** a
   direct read (`fetch_reference.rs:128`), substituting ADR-065 stage containment.
   That substitution is defensible and arguably stronger — the guard applies to
   *every* tool, not just this one — but it can only express "the stage's own
   `references/`". DM's INV-3 is `r ∈ bindings(active())` over multiple named
   corpora, and Ferric has no way to bind 2 of 5 shared corpora. **Either DM
   narrows INV-3 to one-corpus-per-stage, or Ferric grows `target`.**

2. **The zero-marginal-token claim does not hold on the Ferric path.** DM §6.1
   rests it on `ttlMs`/`cacheScope` making repeat reads free. Ferric re-reads and
   re-chunks the entire vault on every call and re-emits full chunk text each
   time. The 97.5% number is a *first-fetch* measurement and it is real; the
   *repeat*-fetch property is not implemented.

3. **DM's verifier cannot see any of this — and is weaker than s81 described.**
   `test_ferric_citations_resolve` (`verify-spec.sh:222-228`) checks only that
   `crates/ferric-icm/src/lib.rs` and `crates/ferric-loop/src/grammar.rs` *exist*.
   **Neither is the file implementing the seam** — the check would pass if
   `fetch_reference.rs` had never been written. Worse, when the Ferric repo is
   absent it calls `pass` on a skip (`:230`), so it is green in CI by
   construction. If the seam is to stay honest across two repos, that check must
   assert the actual `input_schema` in `fetch_reference.rs` against the descriptor
   in `INTEGRATION.md`, and must fail (not skip-pass) when it cannot run.

---

## 7. Operational

- **F1 — resolved.** `C:` was at 241 MB free in s81; the user cleared `target/`,
  and the volume now has **69 GB** free. Cold rebuild takes 31s.
- **F2 — `refs/ferric/*` snapshot refs accumulate with no GC.** Correctly
  namespaced (zero snapshot commits reachable from `main`), but nothing prunes
  them and each pins a full tree object. Compounds with A3.
- **F3 — `.gitignore` is in good shape.** Root-level `e2e_log_*.txt`,
  `ring0.jsonl`, `report_*.md`, `fleet_*.jsonl`, `job.log`, `toolbench_*.log`
  clutter is untracked and correctly ignored. Only B7 is actually committed.
- **F4 — sprint artifacts above s35 are untracked** (`sprints/` is gitignored,
  s0–s35 predate the rule). The durable record is `README.md` + `decisions.md`,
  which is why this report lives in `docs/`.

---

## 8. Suggested order of work

Sprint 81 put "free the disk" first; that is done, and everything below is now
verified rather than inferred.

1. **A3** — snapshot under a temporary `GIT_INDEX_FILE`. Prevents silent data
   loss, and note the fix is *not* the one the code comment suggests.
2. **A1** — restore `for_model` at the projector boundary, and add the
   cross-crate test that would have caught it.
3. **A2** — taint `summary` + quotes. Makes ADR-044 real for the one path that
   has a taint source.
4. **A4** — de-panic `manage_task`; cover the panic paths and the `send_input`
   race, not just the happy path.
5. **C1** — `run_with_provider(RunArgs)`. Mechanical; removes 5 clippy allows.
6. **B3, B7, C6** — trivial cleanups; B3 is already proven safe to apply.
7. **§6** — decide the `target` question with Dark Matter, and harden its
   verifier, before DM's s2 build sets the other side of the contract.

A1–A4 are four defects that a 463-test green suite does not see. The common
shape: **each one is covered up to a boundary and not across it.** That is the
structural lesson worth carrying into the test strategy.

# Full-Codebase Verification — sprint 85 (round 2)

**Date:** 2026-07-25
**Scope:** `Animus_Ferric` @ `5a39ce3` (main, post-PR #44) — 14 crates — plus
`Animus_Dark_Matter` @ `bef3fd9` (main, post-PR #2).
**Method:** cold clean-room rebuild, full gate, then an audit weighted
deliberately toward **sprints 83–84's own changes** — the newest, least-exercised
code in the tree, written by the same process now checking it.

---

## 0. Baseline — clean-room, fully green

The user ran `cargo clean` first, so this is a genuine cold build.

| Check | Result |
|---|---|
| `cargo build --workspace --all-targets` (cold) | clean, **0 warnings**, 41s |
| `cargo test --workspace` | **503 passed / 0 failed / 2 ignored**, 53 suites |
| `cargo clippy --workspace --all-targets` | **0 warnings** |
| `cargo fmt --all --check` | clean |
| DM `scripts/verify-spec.sh` | **PASS 62 / FAIL 0 / SKIP 0** |

`target/` rebuilt to 2.4 GB; `C:` has 58 GB free. Disk is not a constraint this
round (it was the blocker that stopped sprint 81 entirely).

---

## 1. New findings

Tagged **PROVEN** (a written test failed on current `main`) or **CONFIRMED**
(verified at the cited line). Three of the four are defects *this work
introduced* in sprints 83–84.

### E1 — PROVEN — one tool call prompts the human twice

`--accept-edits` installs an `EditApprover`. `run.rs:411` consults it at the
accept-edits gate for any mutating call. Sprint 84 (A7) then also handed the same
approver to `Registry::execute` as the sink approver (`run.rs:766`). With
`--sink-action requireapproval` and tainted args, **both gates fire on the same
call**.

```
SPRINT85_PROBE approver_prompt_count=2 stop=TaskComplete
```

Beyond the annoyance, the semantics are undefined: a user who approves at one
gate and rejects at the other gets behaviour nobody designed. **Introduced by
sprint 84.** The fix is to skip the sink prompt when the accept-edits gate has
already approved that same call — the two gates ask about the same decision.

### E2 — PROVEN — taint granularity makes `--research` + the default policy unusable

Sprint 83 (A2) set `MIN_TAINT_SEGMENT_CHARS = 12` by judgement, with no
measurement. Measured now, against a research digest of the shape the quarantine
actually produces:

```
SPRINT85_PROBE benign_writes_blocked=3/3
  BLOCKED: The configuration file lives at the repository root.
  BLOCKED: Tests are run with cargo test across the workspace.
  BLOCKED: The project is a Rust workspace with several crates.
```

**Every faithful restatement of researched material is blocked**, under
`SinkPolicy::deny()` — the default. An agent that researches then writes about
what it found cannot write.

**No threshold fixes this, and that is the important part.** Substring taint
tracking cannot distinguish "the model copied an injected instruction" from "the
model wrote a true fact it learned" — both are literal text derived from the
digest. Lowering the floor worsens false positives; raising it lets a lifted
sentence through (which is precisely the case sprint 83 added `taint_text` to
catch). This is an inherent limit of CaMeL-lite substring tracking, not a tuning
error.

So it needs a **decision**, not a patch. Options, roughly in increasing cost:

1. **Default the research path to `Warn`, not `Deny`.** Keeps the signal, stops
   blocking legitimate work. Weakest guarantee, but an unusable control gets
   turned off entirely, which is weaker still.
2. **Taint only imperative/instruction-shaped fragments** rather than all prose.
   Heuristic, and heuristics at a security boundary rot quietly.
3. **Accept it and document that `--research` implies read-mostly** under `Deny`.

Sprint 83's fix remains directionally right — tainting `source` was inverted
against the threat model, and tainting content is correct. The granularity is
what needs the call.

### E3 — CONFIRMED — run and replay disagree about the truncation cap

Sprint 84's A1 commit claimed the projector "keeps run and replay identical by
construction". It does not:

- `run.rs:560` — `TraceProjector::new().with_truncation_limit(args.registry.truncation_limit())`
- `replay.rs:63` — `TraceProjector::new()` (default 4,000)
- `compact.rs:224` — `TraceProjector::new()` (default 4,000)

With a registry configured to a non-default cap, a resumed or compacted session
reconstructs a **different context window** than the one actually sent — which is
the exact dual-maintenance failure sprint 44's projector refactor existed to
abolish.

**Latent, not live:** `Registry::with_truncation_limit` still has no non-default
caller (it was sprint-82 finding B2, and sprint 84's plumbing gave it its first
real consumer without giving it a user). It becomes live the moment anyone
configures a custom cap.

Recorded as CONFIRMED rather than PROVEN deliberately: `TraceProjector` is not
exported from the crate root, so this cannot be probed from an integration test.
That non-export is itself corroboration — and writing an in-crate unit test is
the fix's job, not the audit's.

### E4 — CONFIRMED — `ferric chat` silently discards trace-write failures

`ferric-trace`'s `write_event` returns `Result` and genuinely can fail
(`write_all`, `flush` — `sink.rs:50-52`). `run.rs` propagates it with `?` at **21
sites**. `chat.rs` discards it at **all 6** — `SessionStart` (437), four `Note`s
(493, 524, 561, 567), and `SessionEnd` (581).

So a chat session's trace can be silently incomplete: a full disk or a locked
file loses events with no signal. For a project whose stated thesis is *"if it
isn't in the trace, it didn't happen,"* and whose sink is deliberately
flush-per-event for durability, discarding the durability result is the wrong
default.

**Not from sprints 83–84** — this predates the audit work entirely.

---

## 2. Classes swept clean

Worth stating positively, because these are the classes the previous two rounds
kept finding, and they are now closed.

- **The A4 panic class.** Production (non-test) `unwrap`/`expect` across
  `ferric-tools`, `ferric-loop` and `ferric-guard` is down to **6 sites**, all
  safe idioms: a constant `Regex::new(...).unwrap()` (`grammar.rs:25`), two
  capture-group `unwrap`s guaranteed non-optional by a successful match
  (`grammar.rs:29-30`), and two `child.stdout/stderr.take().unwrap()` immediately
  after setting `Stdio::piped()` (`shell_exec.rs:144-145`). No model-driven path
  can panic the harness.
- **The A7 "built but never introduced" class.** No unresolved
  `not wired` / `not implemented` / `TODO` / `FIXME` intent comments remain in
  any crate's source. The only hits are the explanatory comments describing
  fixes already made.
- **Stale ledger entries.** All seven pre-audit backlog items still reference
  files that exist (`ferric-bench/src/verify.rs`, `ferric-provider/src/stream_scan.rs`,
  `ferric-cli/src/{config,backend}.rs`, `animus-launch/src/lib.rs`). None are dead.

---

## 3. Still open from sprint 82 — re-verified, unchanged

| # | Item | Measured now |
|---|---|---|
| C7 | `ferric-cli` is one oversized crate | **9,674 lines across 19 flat modules** |
| C8 | Test runners scattered | **6 scripts, 3 locations, 2 shell dialects** (`e2e_test.ps1`, `run-tool-sweep.ps1`, `run_benchmarks.ps1`; `tools/run-{coverage,e2e}.sh`; `workspace/run-e2e-sweep.sh`) |
| B1 | `Protocol::{FencedCode, EditFormat}` | still at `scale.rs:56-57`; removal is a trace/profile **schema change**, not a deletion |
| — | DM return envelope | DM SPEC §6.2 `{chunks:[{uri,text,score}], truncated}` vs Ferric's markdown; needs an A/B, unchanged |

These three were never entered in `agent-tasks/` — they lived only in a README
"Next" line. **That is itself a process finding:** work that is only mentioned in
prose is work that quietly evaporates. They are entered in the backlog now.

---

## 4. The structural gap: nothing has met a real model since ~sprint 26

All **503 tests are mock-driven.** Every capability claim in sprints 82–85 rests
on `MockProvider` scripts and static analysis. Specifically unexercised:

- **A5's sandbox has never run against Docker** — only `docker_args()` is tested,
  because Docker is not installed on this machine. The flags are verified; the
  container is not.
- **A1's truncation, A2's taint, A6's tokenizer** are all measured against
  synthetic inputs. A6's `MIN_SUBSTRING_TERM_CHARS = 3` is the same
  asserted-not-measured shape as E2's threshold, and has had no real-vault test.
- **The whole fleet capability map** (ADR-035's tiers, `measured_level`) dates
  from sprint 25–26.

This is not a defect, but it bounds what the green suite means. **A suite this
green, this long without a live run, is measuring the mocks as much as the code.**
A live-model round is the single highest-value next investment.

---

## 5. Suggested order

1. **E1** — one call, one prompt. Small, and it is a regression this work
   introduced.
2. **E4** — propagate trace-write failures in `chat.rs`, matching `run.rs`.
3. **E2** — *decide* the taint posture (option 1/2/3 above). Not a patch.
4. **E3** — seed the replay/compact projectors from the same cap, and delete the
   over-claim from ADR-074's wording.
5. **A live-model round** (§4) — worth more than C7/C8/B1 combined.
6. C7 / C8 / B1 — organisational; no behaviour.

---

## 6. Honest note on this round

Three of the four new findings are defects introduced by sprints 83–84, and two
of those (E1, E2) sit in the security-facing code those sprints were fixing.
Sprint 84's own commit message asserted the run/replay property that E3 refutes.

That is the argument for auditing your own recent work first, and for the
distinction this report keeps between PROVEN and CONFIRMED: **the previous two
rounds' green suites did not catch any of this, and neither did the commit
messages that claimed it.**

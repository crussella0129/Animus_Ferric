# Sprint 82 — Research Report

## 1. Goal

Verify the entire codebase and all 14 crates; classify code as critical/effective,
refactorable, or vestigial. Observe and verify the Animus Dark Matter seam
(`fetch_reference` vs the MCP knowledge-layer fold).

## 2. Primary input

`sprints/s81/verification-report.md` — a complete static audit that could not run
`cargo test`, `clippy`, or `fmt`: the `C:` volume was at 100% (241 MB free) and a
45-minute `cargo test --workspace` wedged with zero linker progress. s81 named its
own unblocking step (`cargo clean`) and deferred both remediation and its ADR.

**The user performed that `cargo clean` before this sprint.** `C:` now has 69 GB
free. This sprint is therefore not a re-audit — it is the empirical half s81 was
structurally prevented from doing.

## 3. Method

For each s81 finding: re-derive at the cited `file:line` against current `main`
(`dc15da3`), then — where the defect admits it — **write a test that fails on
current `main`**. Probes were run, output recorded, and deleted; the tree ends
unmodified. Findings are tagged PROVEN / CONFIRMED / CORRECTED.

## 4. Findings

Full report (durable, tracked): **`docs/verification-2026-07.md`**. Summary:

### Baseline — every s81-blocked check now runs

| Check | Result |
|---|---|
| Cold `cargo build --workspace --all-targets` | clean, 0 warnings, 31s |
| `cargo test --workspace` | **463 passed / 0 failed**, 52 suites, exit 0 |
| `cargo clippy --workspace --all-targets` | 0 warnings, exit 0 |
| `cargo fmt --all --check` | clean, exit 0 |
| DM `scripts/verify-spec.sh` | PASS 61 / FAIL 0 |

484 `#[test]` attributes; 463 execute, 2 `#[ignore]`d, rest `cfg`-gated.

### Defects — 3 PROVEN by failing test, 4 CONFIRMED by inspection

| # | Status | Evidence |
|---|---|---|
| A1 tool-output truncation dead | **PROVEN** | `longest_user_message_chars=20028` vs ADR-002's 4,000 — 5× overrun |
| A3 git index destroyed per turn | **PROVEN** | `staged_before="staged.txt" staged_after=""` |
| A6 short-token queries find nothing | **PROVEN** | `query="Go"` → "No reference chunk matched" over an all-Go vault |
| A2 taint set tracks wrong value | CONFIRMED | `query.rs:927-928` taints `source`, injects `summary` |
| A4 `manage_task` can panic harness | CONFIRMED | 9 unwraps + 3 in `builtin/task_registry.rs` |
| A5 sandbox airlock off by default | CONFIRMED | `enforce_runsc:false`, `proxy_url:None`, `--network bridge` |
| A7 `RequireApproval` → `Deny` | CONFIRMED | `registry.rs:207`, while `EditApprover` shipped in s79 |

### Corrections to sprint 81

1. **s81's fix for A3 is wrong.** It endorsed the source comment's
   `git read-tree HEAD`; measured, that destroys the staged set identically to
   `git reset`. The verified fix is a temporary `GIT_INDEX_FILE`, which preserves
   the index *and* still captures untracked files.
2. **A4's "this file has zero tests" is wrong.** Zero *inline* tests, but
   `tests/background_tasks.rs` covers spawn→list→status→kill. Accurate statement:
   happy path covered, every panic path and the `send_input` race uncovered.
3. **A4's `task_registry.rs` path was cited one directory too high** — it is
   `crates/ferric-tools/src/builtin/task_registry.rs`.
4. **DM's verifier is weaker than s81 described.** `test_ferric_citations_resolve`
   checks that two files *exist*, neither of which is `fetch_reference.rs`, and
   `pass`es on skip when the Ferric repo is absent.

### Vestigial / refactor / unreachable

B1–B8 all confirmed; **B3 proven** — the 6 unused deps were actually removed and
`cargo check --workspace --all-targets` exited 0, then reverted. B4 needed
precision: the dead thing is the `LoopState` *field*, not the same-named local.
C1–C8 confirmed (`run_with_provider` = 18 positional params, 5 clippy allows;
`ferric-cli` = 9,689 lines / 19 flat modules; `ferric-vcs` async fns contain zero
`await`). D1–D3 confirmed unreachable from the binary.

### Dark Matter

Spec-only, 17 files, verifier green. Ferric's side works and is measured (97.5%
first-fetch prompt reduction, ADR-071). **But the contracts are incompatible**,
proven both halves: a DM-schema-legal call (`{"target":"qwen-docs"}`) returns
`Err("missing required string argument: query")`, and Ferric returns markdown
where DM specifies `{chunks:[{uri,text,score}], truncated}`.

## 5. Conclusion

The load-bearing core is genuinely good and genuinely covered. The defects are
not sloppiness — they cluster on one structural shape: **each is covered up to a
crate boundary and not across it.** A1's Registry end is tested and correct; the
loop end that discards the value is untested. That is the lesson worth carrying
into test strategy, and it is why 463 green tests coexist with four live defects.

Scope was audit-only by design; remediation is ordered in
`docs/verification-2026-07.md` §8, led by A3 (silent data loss) then A1.

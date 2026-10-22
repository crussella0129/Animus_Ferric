# Plan Critique — Sprint 126

## Concerns

### C-001: `ferric-bench` named as a consumer, but it uses none of the moved items
- **Where:** `build-plan.md` T-12601 acceptance criterion
- **Quote:** "`ferric-loop` and `ferric-bench` use the core"
- **Failure mode:** intent-drift
- **Why it matters:** A grep of `crates/ferric-bench` finds no use of `action_schema`, `parse_json_action`, `select_protocol`, `ToolDescriptor`, `ConstrainedJsonScanner` or `control_descriptors`. The actual consumers are `ferric-loop`, `ferric-provider` and `ferric-cli` (`human.rs`, `query.rs`, `toolbench_cmd.rs`). An acceptance statement about a crate that has nothing to rewire is vacuous and hides the real touch set.
- **Suggested response:** fix-in-plan. T-12601 now names the real consumers and records that `ferric-bench` has no dependency on the moved items. INT-0011 AC-2's wording is unchanged, because it holds trivially for `ferric-bench`.

### C-002: No clause for an upstream HTTP error or an unreachable upstream
- **Where:** `build-plan.md` T-12606
- **Quote:** "WHEN a constrained upstream completion does not parse as an action … 502"
- **Failure mode:** missing-risk
- **Why it matters:** INT-0012's honest-failure boundary covers an upstream that rejects the constraint. llama.cpp rejects an unsupported schema with a non-2xx status, which is the most likely real failure with Hermes's tool schemas. Without a clause, the valve could surface a confusing empty or partial response.
- **Suggested response:** fix-in-plan. Added an EARS clause (non-2xx or unreachable leads to 502 with the upstream status and a bounded body excerpt, with no retry) and two integration tests: `upstream_http_error_yields_502_with_status` and `upstream_unreachable_yields_502`.

### C-003: A single prefix hash cannot prove that a prefix "extends" the previous one
- **Where:** `build-plan.md` T-12605 last clause, T-12607 receipt fields, T-12608 readiness clause
- **Quote:** "`prefix_hash` sequence in which each request's rendered prefix extends the previous request's"
- **Failure mode:** EARS-vague
- **Why it matters:** One hash over all rendered messages changes on every turn by construction, so two receipts' hashes cannot show that one request's rendering is a prefix of the next. As written, the clause is untestable from content-free receipts.
- **Suggested response:** fix-in-plan. Receipts gain `message_hashes`, one SHA-256 per rendered upstream message and still content-free. Extension is now defined as the previous list being a prefix of the current list. The T-12605, T-12607 and T-12608 clauses and their tests reference it.

### C-004: The runner's RAM admission check depends on a probe it cannot reach
- **Where:** `build-plan.md` T-12608
- **Quote:** "runs an admission check for available RAM against the model, reusing `ferric-core` fit (Sprint 122)"
- **Failure mode:** hidden-dep
- **Why it matters:** `ferric-core::fit` only estimates and classifies. The system-memory probe is `pub(crate)` in `crates/ferric-cli/src/startup/memory.rs`. The valve example cannot reach it without depending on the frozen god crate, and duplicating the platform FFI would fork the code.
- **Suggested response:** fix-in-plan. T-12608 first moves the probe, unchanged, into `ferric-process` as a public module in its own behavior-preserving commit. `ferric-process` already carries the platform `windows-sys` and `libc` dependencies, and the runner needs it for `ProcessTree` anyway. `ferric-cli` then uses the probe from there. The moved `parse_meminfo` tests and the existing CLI fit and picker tests prove the move. An unknown probe result is also refused (`admission_refuses_unknown_or_insufficient_memory`).

### C-005: README edits can collide with existing doc-guard tests
- **Where:** `build-plan.md` T-12610
- **Quote:** "Docs only. The template-hygiene test must still pass."
- **Failure mode:** hidden-dep
- **Why it matters:** `crates/ferric-cli/tests/` contains doc guards (`human_docs.rs`, `source_execution.rs`, among others) that assert README and guide content. A retarget that removes asserted strings would fail CI, and "fixing" the guard would weaken it.
- **Suggested response:** fix-in-plan. The T-12610 notes now require the existing guards to pass unchanged and forbid editing them. The text changes work around what they assert.

### C-006: The pilot does not exercise a Hermes-sized tool catalog
- **Where:** `build-plan.md` T-12608/T-12609 ("the `file` toolset only") versus INT-0013 AC-2 ("choosing the single correct tool from a catalog as large as Hermes's")
- **Quote:** "the `file` toolset only, so no model-authored shell runs"
- **Failure mode:** missing-risk
- **Why it matters:** The research named large-catalog prompt and grammar-compile cost as a risk. A file-only pilot will not measure it.
- **Suggested response:** defer-with-rationale. The file-only restriction is a safety boundary the owner approved (no model-authored shell). INT-0013 AC-2 is not claimed this sprint; the build plan's Intents section scopes INT-0013 to AC-1 and a pilot slice of AC-3. The pilot report must state the catalog size used and list the large-catalog measurement as open, and INT-0013's planned transition already says the other ACs remain open.

### C-007: Canonical history re-rendering may diverge from the model's generated bytes
- **Where:** `build-plan.md` T-12605 history projection; INT-0012 "Deterministic prompt bytes" boundary
- **Quote:** "the compact canonical action `{"thought":<reasoning_content or "">,…}`"
- **Failure mode:** intent-drift (screened; not a violation)
- **Why it matters:** INT-0012 promises deterministic rendering between requests, which the plan meets and tests. It does not promise that the re-rendered assistant turn equals the tokens the model actually generated. The two can differ in whitespace, or by losing a thought on final answers. On the hybrid Qwen3.8 model, that difference can force a KV checkpoint rollback, which would make the valve arm look slower for rendering reasons, not grammar reasons.
- **Suggested response:** defer-with-rationale. This is the "measure before trying something else" case the owner asked for. Receipts record `cached_tokens` per request, so the pilot report quantifies the cost per turn and attributes it separately. Exact-byte echo is a follow-on only if the cost is material. Both arms use identical server flags, including checkpoint settings.

### C-008: Two "tests" are evidence checks rather than executable tests
- **Where:** `test-plan.md` rows `suite_totals_unchanged` and the T-12602 measurement rows
- **Quote:** "`suite_totals_unchanged` (recorded baseline vs post-move totals…)"
- **Failure mode:** plan-test-mismatch (screened, accepted)
- **Why it matters:** A suite-count comparison and a measurement record cannot be unit tests without testing the test runner itself.
- **Suggested response:** accept. The Test phase verifies them as recorded artifacts, with baseline totals at `bf3c7d8` against post-move totals, and both numbers come from the same canonical command. This matches how previous sprints evidenced behavior-preserving moves (INT-0009 AC-3).

### C-009: A Python file enters a Rust repository without a lint gate
- **Where:** `build-plan.md` T-12608 (`e2e/hermes_driver.py`)
- **Quote:** "plus `e2e/hermes_driver.py`"
- **Failure mode:** granularity (screened)
- **Why it matters:** The global instructions require `ruff format` and `ruff check` for Python edits, and CI does not lint Python here.
- **Suggested response:** fix-in-plan. The T-12608 notes now require the driver to pass `ruff format --check` and `ruff check`, and the Test phase records the result.

### C-010: The enumerated re-export list is incomplete
- **Where:** `build-plan.md` T-12601 first clause
- **Quote:** "`ferric_loop::{action_schema, parse_json_action, …, request_of}`"
- **Failure mode:** EARS-vague
- **Why it matters:** `ferric-loop`'s `lib.rs` also exports `UserInputRequestError` and other items from the moved modules. A hand-written list invites a silent omission.
- **Suggested response:** fix-in-plan. The clause now covers every item exported from the moved modules at `bf3c7d8`, and the workspace compile gate plus `reexport_paths_resolve` verify it.

## Confidence
proceed-with-caveats

All fix-in-plan concerns (C-001 to C-005, C-009, C-010) are applied to both
plans. The two deferrals (C-006, C-007) are measurement boundaries the owner
chose, and each is recorded in an intent's state or in the pilot's required
report content. No acceptance criterion is weakened. Stage B remains gated on
the extraction measurement (T-12602), as the owner asked.

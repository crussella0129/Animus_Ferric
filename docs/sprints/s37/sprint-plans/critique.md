# Plan Critique — Sprint 37

(Critic: foreground `Agent` tool, adversarial review against `prompts/plan-critic.md`, specifically
tasked with checking whether the `"tool":"<name>"` substring-scan approach could false-positive.)

## Concerns

### C-001: T-3702's escape-boundary algorithm under-specified for `\uXXXX` (multi-char escapes)
- **Failure mode:** algorithm-gap
- **Response:** **fix-in-plan.** T-3702's description now states the precise withhold rule: hold
  back from the START of any incomplete escape at any of its 6 possible cut points, not just "one
  trailing byte." Added test `ambiguous_trailing_unicode_escape_is_held_back` covering every split
  point of a `\uXXXX` sequence.

### C-002: `"tool":"<name>"` substring-scan safety against false-positive matches was asserted, not justified
- **Failure mode:** algorithm-gap (documentation gap — the critic independently verified the claim
  is TRUE by reading `run.rs`/`grammar.rs`, but the plan never stated why)
- **Response:** **fix-in-plan.** T-3702 now states explicitly why this is safe: the scanned text is
  always the bare top-level action object (never nested), and `action_schema` always emits `tool`
  as the first property (ADR-016) — so no `args` string value can precede and be misread as the
  tool name. Added regression test `tool_field_always_precedes_args_content` pinning this argument
  itself, not just relying on it being true today.

### C-003: No plan for a scanner-streamed-then-discarded turn (truncation mid-summary)
- **Failure mode:** missing-risk
- **Response:** **defer-with-rationale**, per the critic's own suggested framing. T-3702 now
  records this explicitly as a known, accepted rough edge for this increment (narrow — ADR-018's
  token budgets make mid-summary truncation unlikely — and cheap to fix later if it proves
  disruptive), rather than leaving it silently unaddressed.

### C-004: T-3703's "Cargo.toml: tokio gains net" reads as a production dependency change; it's actually test-only
- **Failure mode:** hidden-dep
- **Response:** **fix-in-plan.** T-3703 now specifies precisely: `reqwest`'s `stream` feature is
  the real production need (`.bytes_stream()`); `tokio`'s `net` feature is dev-dependency-scoped,
  needed only by T-3703's own fake-server E2E test, not `complete_streaming`'s production path.
  Keeps ADR-004's allowlist discipline honest (now cross-referenced in the research report too,
  see C-010 below).

### C-005: T-3703 bundles pure logic + async I/O + a Cargo.toml change under one task, citing an unverifiable sprint-36 precedent
- **Failure mode:** granularity
- **Response:** **defer-with-rationale** (keep as one task), per the critic's own suggested
  resolution. Dropped the unverifiable T-3605-precedent claim; T-3703's description now justifies
  the bundling on its own merits — one coherent unit of review over one file's one new capability;
  splitting further would create artificial dependency chains.

### C-006: Retry/backoff semantics for streaming — a research-identified risk with no EARS clause or test
- **Failure mode:** missing-risk (the exact risk research-report.md's Section 4 flagged, absent
  from the plan)
- **Response:** **fix-in-plan.** Added an explicit EARS clause to T-3704 ("a retryable mid-stream
  error SHALL retry with a fresh request and SHALL NOT replay already-fired deltas") and a matching
  test, `streaming_retry_does_not_replay_failed_attempt_deltas`.

### C-007: Default impl's `ToolNamed` behavior for tool-calling completions was unstated
- **Failure mode:** EARS-vague
- **Response:** **fix-in-plan.** Added an explicit EARS clause to T-3701 (default impl never fires
  `ToolNamed`, only `OpenAiProvider`'s real implementation does) and a matching test,
  `default_complete_streaming_never_fires_tool_named`.

### C-008: T-3706 (ADR-047)'s single EARS clause is barely more measurable than "performs well"
- **Failure mode:** EARS-vague
- **Response:** **reject**, per the critic's own assessment. ADR-writing tasks across this
  project's history (T-3506, T-3607) are consistently documentation-only with exactly this level
  of EARS looseness — holding T-3706 to a stricter bar than established precedent would be
  inconsistent, and the critic itself recommended not blocking on this.

### C-009: The fake-server E2E's hand-rolled HTTP framing correctness burden was understated
- **Failure mode:** e2e-drift (a practical implementation-risk warning, not a rejection of the
  approach)
- **Response:** **fix-in-plan.** Added the specific framing requirement to both T-3703 and the
  E2E test description: `Connection: close` (not `Content-Length`, since an SSE body is
  unbounded-length) — avoids an avoidable stall/hang discovered mid-build instead of up front.

### C-010: ADR-004 (the dependency allowlist ADR) missing from Decisions Reviewed despite governing T-3703's Cargo.toml change
- **Failure mode:** ignored-ADR
- **Response:** **fix-in-plan.** Added ADR-004 to research-report.md's Decisions Reviewed section
  with a relevance note tying it directly to T-3703's feature-flag additions and their scoping
  (production `reqwest` stream vs. test-only `tokio` net), in the same spirit as ADR-016/045's
  prior allowlist-amendment precedent.

## Confidence

**proceed-with-caveats → all 10 concerns addressed above (8 fix-in-plan, 1 defer-with-rationale,
1 reject with stated reason).** Plans are ready to lock.

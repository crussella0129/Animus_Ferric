# Test Critique — Sprint 122

## Concerns

### C-001: The won't-fit gate is proven at the decision boundary, not through a full picker driver
- **Where:** `e2e-tests.md` Status / `test-plan.md` End-to-End (`wontfit_session_gate_end_to_end`)
- **Quote:** "a single driver that runs `choose_model`/`session_with` with a scripted terminal and a *chosen* model size is not-yet-possible without a new test seam"
- **Failure mode:** e2e-cop-out
- **Why it matters:** the SHALL responses are all asserted over the real production helpers, but the ~4 wiring lines in `choose_model` (annotate each line; prompt on WontFit; decline → `Ok(None)`) are not themselves executed by a test, so a wiring regression there would not fail the suite.
- **Suggested response:** defer-with-rationale. Model `bytes` come from real on-disk metadata (`models::scan`) and `Startup::begin_in` is private, so a test cannot present a 20 GiB model without a 20 GiB file — a genuine infrastructure gap, not a skipped assertion. The wiring is byte-for-byte the same `io.read` y/N pattern already proven by `human_work_requires_scoped_consent`/the source-tree guard. Recorded as a named follow-up (a `#[cfg(test)]` `Startup` seam or an injectable `MemoryProbe` in `session_with`). Not a `block`: no intent criterion or EARS SHALL is unproved.

### C-002: (carried from Plan) fit keys on available, not total — verified
- **Where:** `unit-tests.md` T-12203 / `fit.rs` `classify_fit`
- **Quote:** "`fit_keys_on_available_not_total` — 64 GiB total / 2 GiB available with a 10 GiB model → WontFit"
- **Failure mode:** weak-assertion (screened, resolved)
- **Why it matters:** a fit computed against total would re-open a softer version of the human-test trap.
- **Suggested response:** none — the added test pins the classifier to available; concern closed.

## Confidence
proceed-with-caveats

Every INT-0008 AC-13 hardware-informed-recommendation EARS clause maps to a named, executed, tightly-asserted test, including the negative paths (missing `MemAvailable` → `None`, unmeasured → `Unknown` and the verbatim legacy line). The single caveat (C-001) is a real front-door test-seam limitation with a named unlock, not an unproved promise.

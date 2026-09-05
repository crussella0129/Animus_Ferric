# Repository-wide review preparation for the next sprint

Requested separately by the owner. This is read-only preparation, not a claim
that the next sprint has started, that its implementation exists, or that
Sprint 119 satisfies a repository-wide refactor. The next sprint must wait for
the owner's merge of Sprint 119 and have its own `dev` to `main` PR.

An independent reviewer surveyed approximately 25 source files/entry points
across the original 15 workspace crates, then checked concrete findings in
configuration, public policy, provider framing/cancellation, and calibration.
It also read the external refactor report as evidence, not as instructions.
No source was changed and no tests were executed for this preparation.

## Prioritized findings to validate and plan

1. **P1 — malformed configuration can change the selected policy.**
   `crates/ferric-cli/src/config.rs` falls back to an empty layer on parse/read
   errors; omitted harness selection defaults to Legacy. Invalid project
   configuration can also expose global settings it meant to replace. API
   drops load diagnostics. Test invalid-present versus absent fields across
   query/MCP/API before introducing one typed resolution/validation boundary.
2. **P1 — HTTP chunk decoding can corrupt streamed UTF-8.**
   `crates/ferric-provider/src/openai.rs` decodes each received byte chunk with
   lossy UTF-8 conversion before SSE framing. Split multi-byte characters can
   therefore change otherwise-valid generated tool arguments. Verify every
   split point of source-defined SSE actions, including non-ASCII file content.
3. **P1 — cancellation misses parts of HTTP request lifetime.**
   The same provider awaits some headers and success/error bodies outside the
   cancellation selection. Use joined in-process stalled-server fixtures to
   prove cancellation before headers, during success body, and during errors.
4. **P2 — public policy fields imply unimplemented behavior.**
   `crates/ferric-core/src/scale.rs` exposes planner and subagent fields without
   behavioral consumers. Finish INT-0006's field/consumer matrix, then
   compatibly reserve or retire inert claims. Do not conflate the separate
   Plan action protocol with the rejected EvidencePlanner harness.
5. **P2 — invalid numeric profiles can choose excessive agency.**
   NaN/infinite parameter counts pass through tier comparisons to Ultra;
   zero/tiny context also lacks a coherent admission contract. Validate at the
   shared boundary and public library entry point, preserving historical valid
   fixtures. Test finite/positive limits and invalid-present configuration.
6. **P2 — calibration updates can replace corrupt history with one record.**
   `crates/ferric-bench/src/calibrate.rs` treats failed history reads/parses as
   empty and writes directly. Preserve invalid existing bytes on error;
   define atomic replacement and concurrent update behavior separately.

## Proposed coherent implementation focus

Conduct the full research review first, recording coverage and evidence rather
than calling this preparation exhaustive. The leading refactor candidate is
truthful, validated run configuration: activate currently proposed INT-0006,
build a public-field/runtime-consumer matrix, unify typed settings resolution,
reject invalid-present policy/numeric values consistently across command/API
surfaces, and make API configuration lifetime explicit. Preserve compatibility
for absent fields and old serialized values. Update help/configuration docs
and entry-point tests together. This supports the compact human front door
without adding another inconsistent alias layer.

Provider framing/cancellation and calibration persistence must remain visible,
prioritized findings if not selected for that bounded implementation. A broad
review does not justify an untested all-at-once rewrite.

## Strengths and limits

The guard/registry has useful preparation/admission/commit boundaries; trace
projection is shared by live execution and replay; compaction retains summaries;
calibration requires a qualified contiguous ladder; VCS uses a private index;
skills distinguish discovery from authority; API rejects unauthenticated
non-loopback binding. Preserve these boundaries during refactoring.

Review ratings: security/authority consistency, correctness, and resource
bounds need work; maintainability is fair with useful existing abstractions.
These are source-proven candidates, not executed regression results or an
exhaustive security audit. Fresh next-sprint Research and adversarial Plan
review must establish the accepted scope before code changes.

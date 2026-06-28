# Plan Critique — Sprint 30

> Self-critique against `prompts/plan-critic.md` (no subagent spawn — autonomous loop, mid-pivot).

## Concerns

### C-001: Mid-sprint pivot — is the research grounded?
- **Failure mode:** thin-research
- **Response:** the goal was *recovered*, not invented: Ornstein is fully specified in the s1
  artifact (`docker-nix-tailscale.md`) + ADR-014 roadmap. The research report cites it and the
  original CaMeL/Willison sources. Grounded.

### C-002: "Quarantine" might be security theater (a prompt, not a guarantee)
- **Failure mode:** false-assurance
- **Response (the core design choice):** the quarantine is **structural** — `empty tools` +
  a **data-only** output schema + single-shot. ADR-010's `CompletionRequest::validate` makes
  empty-tools the *only* valid constrained shape. The injection-containment test asserts the
  type has **no** action channel, so untrusted content cannot become a tool call regardless of
  what it says. This is the real property, tested, not a system-prompt plea.

### C-003: Scope — Ornstein is huge; is increment 1 coherent on its own?
- **Failure mode:** over-scope / orphan-increment
- **Response:** increment 1 is the *primitive* every later layer needs (the typed quarantined
  output). It's independently valuable (a reusable `summarize_quarantined`) and testable. The
  deferred layers (container/gVisor, allowlist proxy, CaMeL sink-policy, Loop wiring) are
  enumerated in ADR-040 so they can't evaporate — directly fixing the failure that left Ornstein
  unbuilt since s1.

### C-004: Provenance laundering
- **Failure mode:** trust-the-model
- **Response:** `source` + `untrusted` are **stamped by the harness after parsing**, overwriting
  whatever the model emitted — asserted by the provenance test. The model can't clear its own
  taint.

### C-005: New crate — workspace churn
- **Failure mode:** build-break
- **Response:** additive (new member + dep entry), mirroring the existing `ferric-*` crate
  pattern. `cargo test --workspace` proves it joins cleanly. No existing crate changes.

## Confidence
`clean` — a small, additive, well-scoped crate that turns the project's proven core mechanism
(constrained decoding) into a security primitive, with a structural (not prompt-based)
guarantee asserted by a deterministic injection-containment test. The large remainder of
Ornstein is explicitly sequenced, not skipped.

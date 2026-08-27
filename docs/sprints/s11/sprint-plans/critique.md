# Plan Critique — Sprint 11

> Self-critique against `prompts/plan-critic.md`.

## Concerns

### C-001: The ADR-020 hang might recur when a real constraint is passed
- **Failure mode:** known-risk / unrecoverable-loop
- **Response:** **accept + contained.** The probe runs as a `timeout`-bounded subprocess, so a hang is killed externally and *is itself* a recordable outcome (case c). The capability flip is gated on enforcement-without-hang, so a hang never reaches `ferric query` users. This is exactly the ADR-020-safe harness the probe was built for.

### C-002: Capability advertised before it's proven would route the loop into a hang
- **Failure mode:** premature-capability
- **Response:** **designed against.** `supports_constraint` stays `false` through the build; it flips to `true` ONLY in the loop phase, ONLY on probe case (a). Until then `select_protocol` keeps mistral.rs on `TextXml` exactly as today — zero behaviour change for users this sprint until the win is verified.

### C-003: One build task is thin
- **Failure mode:** granularity
- **Response:** **accept (correct).** The whole sprint is a *spike*: the code change is genuinely one focused wiring (a mapping fn + one `set_constraint` call); the value is the empirical probe result + the ADR-027 decision. Splitting would be artificial.

### C-004: mapping test can't compare values (no `PartialEq` on `mistralrs::Constraint`)
- **Failure mode:** weak-assertion
- **Response:** **accept.** `matches!` on the variant is the right level — the mapping is a 3-arm variant-to-variant copy; the payload is moved through unchanged. The real correctness proof is the E2E probe enforcing the actual schema.

## Confidence
`proceed-with-caveats` — tiny, reversible code change behind a bounded probe; the only real risk (hang) is contained by `timeout` and gated out of the user path.

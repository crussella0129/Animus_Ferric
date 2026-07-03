# Plan Critique — Sprint 35

> Self-critique against `prompts/plan-critic.md` (audit-driven sprint; no subagent spawn — the
> background review agents were stopped by the user; this is a direct, code-verified audit).

## Concerns

### C-001: Four unrelated fixes in one sprint — scope discipline?
- **Failure mode:** sprawl
- **Response:** each is small, independent, and individually testable (no shared code path
  between the guard change, the server flags, and the two Cargo.toml dependency changes); bundling
  them as "the refactor" matches the sprint's own stated shape (review AND refactor, not one
  narrow feature). The genuinely large/risky items (sink-policy wiring, MCP, chat mode, shell/git
  tools) were explicitly excluded and deferred with reasons — the discipline is in what's NOT
  included, not the count of small items that are.

### C-002: Read-side denylist — false positives / over-blocking?
- **Failure mode:** breaks legitimate workflows
- **Response:** deliberately narrower than the write list — `.git` is excluded from
  `DENIED_READ_SEGMENTS` specifically because reading git metadata for code context is legitimate
  and common; only the credential-store segments (`.ssh`/`.gnupg`/`.aws`/`.kube`/`.ferric`) and
  unambiguous secret filenames are denied. The regression tests (`.git/config` and an ordinary
  file both still `Allow`) are the proof this doesn't overreach.

### C-003: `.env` read-denial — is this actually the right file to add?
- **Failure mode:** wrong-target
- **Response:** `.env` is the single most common real-world secret file in a coding workspace
  (API keys, DB passwords) and was on **no list at all** before this sprint — a bigger gap than
  the SSH-key case the existing tests already cover. Adding it to reads only (not writes) is
  intentional: creating/editing a `.env` as part of normal dev work is legitimate; reading an
  *existing* one with real secrets into context/trace is the risk.

### C-004: `ferric server` new flags — Ollama silently ignoring them
- **Failure mode:** confusing UX
- **Response:** accepted as the pragmatic choice for this sprint — a warning/error path for
  "flag set but ignored" is a UX nicety, not a correctness or safety issue, and adding one would
  expand scope. Noted in the task description; a follow-up can add a warning if it proves
  confusing in practice.

### C-005: `mistralrs` rev-pin goes stale immediately
- **Failure mode:** false-permanence
- **Response:** correct and expected — a pin is a snapshot, not a guarantee of eternal freshness.
  The value is *reproducibility* (every build gets the same commit until someone deliberately
  bumps it), not *staying current automatically*. Matches the project's own existing `oovra`
  policy precisent exactly.

### C-006: `reqwest` TLS swap — could this break something subtle?
- **Failure mode:** regression
- **Response:** Ferric only ever talks to `127.0.0.1` (ADR-005 pins the host), so TLS is barely
  exercised in practice regardless of backend; the swap is evaluated first (not blindly applied)
  and gated on `cargo test --workspace` staying green. If evaluation shows no native-TLS pull is
  actually happening, the task is a no-op, recorded honestly in ADR-045 rather than forced.

### C-007: Is excluding the sink-policy wiring the right call, or should it ship regardless?
- **Failure mode:** under-scoping a security fix
- **Response:** wiring a check that can never fire (no code path sets `tainted=true` yet) would
  be **worse** than not wiring it — it creates the appearance of enforcement with zero actual
  effect, which is a false-assurance risk in its own right. Correct to defer it alongside the
  actual taint-source wiring (the research→loop integration) so both land together and the
  guarantee is real from day one.

## Confidence
`clean` — four small, independent, fully-testable fixes directly serving the stated safe/efficient
goals, each backed by a concrete code-verified finding (not speculation), with regression tests
proving the guard change doesn't overreach and the server-flag change stays backward compatible.
The one item cut from scope (sink-policy wiring) was cut for a substantive reason, not convenience.

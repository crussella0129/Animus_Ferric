# Plan Critique — Sprint 32

> Self-critique against `prompts/plan-critic.md` (user-steered; no subagent spawn).

## Concerns

### C-001: Shipping a retriever whose live path isn't run this sprint
- **Failure mode:** untested-feature
- **Response:** the *security-critical + logic* surface (shell escaping, argv shape, status parse)
  is pure and fully unit-tested; only the process spawn is deferred — exactly the `server.rs`
  precedent (`command()` is tested; the spawn isn't). The user explicitly chose build-now/
  live-test-later (no SSH target reachable). The deferred E2E is documented with the exact command.

### C-002: Remote command injection
- **Failure mode:** RCE
- **Response:** this is *the* concern and the core deliverable. `ssh` always runs its command via
  the remote shell, so the query + root are POSIX single-quote-escaped via `shell_single_quote`,
  with dedicated tests for `'`, `;`, `$(...)`, and backtick payloads. (Defense in depth: the
  content fetched still flows through the quarantine, so even a malicious *file* can't act.)

### C-003: Two transports — scope creep?
- **Failure mode:** over-abstraction
- **Response:** both are real in *this* fleet — `tailscale ssh` for Linux tailnet devices
  (switchblade, keyless) and plain `ssh -p` for the Pixel's Termux sshd. The enum is two variants
  and a branch in the argv builder; it earns its place from the observed environment, not
  speculation.

### C-004: `grep`/`cat`/`head` assumptions on the remote
- **Failure mode:** portability
- **Response:** POSIX/busybox-safe tools (Android Termux + Linux both have them). Flagged as a
  live-E2E verification item; the argv builder is agnostic to the remote toolset's quirks (it just
  builds the command).

### C-005: `available()` shells out to `tailscale status`
- **Failure mode:** hidden-live-call-in-a-probe
- **Response:** `available()` is a capability probe by design (ADR-041); a live `tailscale status`
  is the cheapest honest probe, and `research()` already treats unavailable as a no-op. The
  *parsing* is unit-tested; the one `status` call is bounded + read-only.

## Confidence
`clean` — an additive plane behind the existing keystone, with the security-critical core
(remote-shell escaping) and the parsing/argv fully unit-tested, the spawn deferred per the user
and per the `server.rs` precedent, and the live E2E documented for when a target's sshd is up.

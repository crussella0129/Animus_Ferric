# Sprint 127 Meta

- **Sprint number:** 127
- **Book schema version:** 2
- **Start timestamp:** 2026-10-22T18:15:00Z
- **End timestamp:** 2026-10-22T18:38:35Z
- **Model:** claude-opus-5-5
- **Bundle version:** 0.22.0
- **Exit status:** success
- **Token count:** (filled at Loop Phase if observable)
- **Summary:** Localhost is not a trust boundary (INT-0015, owner direction 2026-10-22, after the 2026 opencode localhost-to-RCE advisories and PHP2Shell's pre-disclosure exploitation). A new `ferric-gate` crate adds four checks in a fixed order: Host (anti-DNS-rebinding), no browsers, a bearer token, and JSON bodies. Every Ferric HTTP listener (`ferric-valve`, `ferric api`) serves through it. `ferric secure probe` and `ferric secure demo` replay the attack classes. `ferric tunnel` provides self-hosted WireGuard on Mullvad's GotaTun with no coordination server, proven end to end across network namespaces.
- **Intents:** [INT-0015](../../intents/INT-0015-localhost-is-not-a-trust-boundary.md), [INT-0012](../../intents/INT-0012-constrained-valve-at-hermes-boundary.md) (the valve's listener)
- **Completion evidence:** INT-0015 AC-1 to AC-9. `ferric-gate` (T-12701), the valve and API served through it (T-12702), `ferric secure probe` and `demo` (T-12703: naive 8/8, gate 0/8), and `ferric tunnel` on GotaTun (T-12704: probe 9/9 across the tunnel, LAN address unreachable). Follow-ups T-12705–T-12709 are open. fmt and clippy are clean; all new tests pass. 19 pre-existing process-lifecycle failures reproduce on the untouched pre-sprint tree in this container (see the [test report](sprint-tests/test-report.md)).

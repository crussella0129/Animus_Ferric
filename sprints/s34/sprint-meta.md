# Sprint 34 Meta

- **Sprint number:** 34
- **Start timestamp:** 2026-06-29T03:20:15Z
- **End timestamp:** 2026-06-29T04:15:00Z
- **Model:** claude-opus-4-8
- **Exit status:** success
- **Token count:** (not observed)
- **Summary:** **Ornstein — the CaMeL-lite sink-policy primitive.** Co-designed with the user: flow control on top of the quarantine. New `crates/ferric-research/src/sink.rs`: `TaintSet` (substring taint over a digest's summary + claim quotes; `args_tainted` recursively walks a tool-call args JSON) + `SinkPolicy::decide(permission: PermissionLevel, tainted: bool) -> SinkDecision` — untainted→Allow, Read+tainted→Allow, Write/Execute+tainted→the configured `SinkAction` (**all 3 modes ship: Deny/RequireApproval/Warn, caller picks**). 8 new tests (29 in the crate) incl. the end-to-end gate shape (a tainted digest's injected quote, echoed into write_file args, is flagged + Denied under the autonomous default). Pure primitive only — not wired into dispatch; the enforcement point (`registry.execute`, beside `check(permission, path)`) is deferred to the research→loop wiring. ADR-044; `docs/ornstein.md`; README Status 34. One PR per sprint; `dev` clean.

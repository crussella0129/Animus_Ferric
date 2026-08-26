# Sprint 36 Meta

- **Sprint number:** 36
- **Start timestamp:** 2026-07-03T14:17:10Z
- **End timestamp:** 2026-07-03T21:40:00Z
- **Model:** claude-sonnet-5 / claude-opus-4-8 (model switched mid-sprint during a Bash-classifier outage)
- **Exit status:** success
- **Token count:** (not observed)
- **Summary:** **`ferric mcp` — the ADR-005 security call, then the MCP-stdio server it unblocks.**
  User-prioritized from the GLM-review "critical gaps" list; the mistral.rs in-process-hang item
  was explicitly dropped (reprobed twice already, ADR-020/027). Shipped exactly one MCP tool,
  `ferric_query` (`{prompt, files?}`) — never Ferric's individual builtins, never
  workspace/backend/model as per-call parameters (all launch-time-fixed `McpArgs` CLI flags, so the
  containment guarantee is structural — the tool schema has no field for them). Every `tools/call`
  runs the same constrained agent loop `ferric query` drives, inheriting the guard/permission
  checks, tool rings, and per-call tracing. Hand-rolled JSON-RPC 2.0 (no `rmcp` dependency — the
  surface is deliberately one tool, no resources/prompts/notifications). Seven build tasks
  (T-3601–T-3607): provider-construction/loop-execution split (`run_with_provider`, reusable);
  launch-time-fixed run-config builder + shared file-routing (`build_run_config`/`route_files`);
  JSON-RPC framing; `initialize`/`tools/list`; the `tools/call` handler (`McpServer`/`Executor`,
  `isError:true` on failure without crashing the serve loop); `McpArgs`/`Command::Mcp`/`run_mcp`
  (one provider + one tokio `Runtime` built once, reused across calls); ADR-046 + docs. A
  foreground plan-critic (7 concerns, all fixed/rejected-with-reason before lock) and a foreground
  test-critic (7 concerns — mostly file-routing coverage/assertion gaps and an E2E hang risk — all
  fixed) both ran mid-sprint. Also reviewed an external "Production Ready Action Plan" doc the user
  supplied mid-sprint; its concrete future-task ideas (streaming via buffer-and-validate, session
  resume, persistent config, shell/git tools, the dev engine, deployment hardening incl. the
  `oovra` supply-chain risk) are captured in `agent-tasks/agent-tasks.md` as reviewed backlog, with
  one noted security divergence (we exposed one agent-loop tool, not tool rings as MCP groups).
  46 tests added/hardened (42 `ferric-cli` lib incl. 20 new + a real-subprocess stdio E2E covering
  a malformed-line negative path); `cargo test --workspace` green; clippy `-D warnings` + fmt
  clean. Chat mode (the other half of the 2026-06-29 ADR-011 revision) remains explicitly
  deferred, needing its own dedicated security-boundary ADR. One PR per sprint; `dev` clean.

# Artifact: OpenClaw Integration Research (mid-2026)

> Source: web-research agent, 2026-06-10. Trimmed; full URL list at bottom.

## Architecture

Gateway-daemon model: one long-lived Gateway per host, WebSocket server on `ws://127.0.0.1:18789` (JSON frames, idempotency keys), same port serves Control UI/WebChat/Canvas. Gateway exclusively owns channels (WhatsApp/Telegram/Slack/Discord/Signal/iMessage/WebChat). Each conversation = a session with dedicated agent + memory, persisted to disk. External tools plug in at four seams: WS API, skills, plugins/bundles, MCP servers.

## Integration surfaces

- **WS/HTTP API**: canonical programmatic surface; shared-secret or identity-header auth.
- **Plugins** (in-process TS, full trust) vs **bundles** vs **skills** (`SKILL.md` + YAML frontmatter; `metadata.openclaw.requires.bins` gates on a binary on PATH — the documented way to wrap an external CLI; `command-dispatch: tool` can bypass the model).
- **MCP first-class**: `openclaw mcp add/probe/tools`; stdio, SSE/HTTP, streamable HTTP transports; tools appear as `serverName__toolName` in coding profiles. OpenClaw can also act as an MCP server.
- **Most natural attachment for a Rust binary: MCP over stdio** — `ferric mcp` registered via `openclaw mcp add ferric -- ferric mcp`. Zero TypeScript, typed contracts, and the same surface works with Claude Code/Codex/Cursor, so Ferric stays standalone-first.

## Remote access + Tailscale

- SSH tunnel to loopback Gateway is the universal pattern; CLI `gateway.mode: remote` with url/token.
- **First-class Tailscale support**: `tailscale.mode: off|serve|funnel`; with serve + `allowTailscale: true`, auth = Tailscale identity headers verified via `tailscale whois`. Community-safe default: loopback bind + SSH tunnel or Tailscale Serve, never public.

## Security taxonomy (arXiv 2603.27517: 470 advisories Jan–Apr 2026)

1. Unauthenticated RCE chain from three moderate bugs → Ferric must do its own authn/authz even "behind" OpenClaw.
2. Exec allowlist lexically broken (shell continuation, busybox multiplexing) → never rely on OpenClaw's command filtering; Ferric enforces its own policy in-binary.
3. Malicious skills ran droppers inside LLM context → keep all capability in the signed Rust binary; SKILL.md stays minimal/auditable.
4. Channel-borne prompt injection (any sender becomes agent input) → treat every request as untrusted; confirmation tokens for destructive ops; never echo credentials into tool results.
5. Per-layer trust, not unified policy → Ferric is the unified policy boundary for its own domain regardless of invoking layer.

Companions: Defensible Design (2603.13151); "Your Agent, Their Asset" (2604.04759).

## Ranked integration shapes

1. **Ferric as MCP stdio server** (build first): `ferric mcp` exposing typed tools (plan/edit/run_task/status). Low-moderate effort (rmcp crate), best safety (no listener, subprocess), works across the whole MCP ecosystem.
2. **OpenClaw SKILL.md wrapping the ferric CLI** (cheap companion): `requires.bins: ["ferric"]`; near-zero effort; weakest alone (rides the broken exec allowlist) — ship alongside #1, not instead.
3. **Ferric daemon as WS/HTTP peer on the tailnet** (later): `ferric serve` + identity-header auth; high effort, owns a listener; defer until #1 proves demand.

Standalone guarantee: all three are additive subcommands on a CLI-first binary.

## Sources

1. https://docs.openclaw.ai/concepts/architecture
2. https://docs.openclaw.ai/cli/mcp
3. https://docs.openclaw.ai/tools/skills
4. https://docs.openclaw.ai/gateway/tailscale
5. https://arxiv.org/abs/2603.27517
Extras: docs.openclaw.ai/gateway/remote; docs.openclaw.ai/plugins/bundles; arxiv.org/html/2603.13151; arxiv.org/html/2604.04759v1; github.com/VoltAgent/awesome-openclaw-skills (5,400+ skills).

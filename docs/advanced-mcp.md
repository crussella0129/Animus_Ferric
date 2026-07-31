# MCP & Animus Dark Matter

Animus speaks the [Model Context Protocol](https://modelcontextprotocol.io) two
ways: it can *be* an MCP server (so an IDE or another agent can call a whole
Ferric run as one tool), and it consumes an MCP-shaped knowledge seam that the
sibling **Animus Dark Matter** project is being built to fill.

## `ferric mcp` — Ferric as one MCP tool

```sh
ferric mcp [OPTIONS]
```

This starts an MCP-stdio server that exposes **exactly one** tool: `ferric_query`,
taking `{prompt, files?}`. It never exposes Ferric's individual builtins, and
never exposes workspace/backend/model as per-call parameters. Those are
**launch-time-fixed** CLI flags held for the server's lifetime.

That restriction is the whole point. Because the wire protocol has no field for
the workspace or the model, an MCP client **cannot** redirect the run to another
directory or swap the model — the containment guarantee is *structural*, not a
policy check that could be forgotten. Every `tools/call` runs the same constrained
agent loop `ferric query` drives, inheriting the guard, the tool rings, the
loop-hardening guards, and per-call tracing.

### Wiring it into a client

Point any MCP-stdio client (an IDE, another agent, Claude Code, Cursor) at the
command. A typical client config entry looks like:

```json
{
  "mcpServers": {
    "ferric": {
      "command": "ferric",
      "args": ["mcp", "--workspace", "/path/to/project", "--model", "your-model"]
    }
  }
}
```

The client then sees a single `ferric_query` tool; calling it runs a full,
contained Ferric agent turn against the fixed workspace and model.

## Animus Dark Matter — the knowledge seam

**Animus Dark Matter** ("Local Intelligence Multiplier") is a separate Animus
Project repository (see [The Animus Project](appendix.md)). Its role is to be a
knowledge layer the harness can query for *just* the context a task needs, rather
than folding whole reference vaults into a prompt.

The seam already exists on the Ferric side, in the [ICM](icm.md) delegation
system. `ferric icm run --fetch-references` composes a stage with its own
`references/` **withheld** and hands the model a `fetch_reference` tool to pull
only the chunk(s) it queries. Measured on a 133 KB reference vault, this shrank a
stage's prompt from 136,162 to 3,355 characters — **97.5% smaller** — with no loss
of the references the stage actually used.

> **Current state vs. intent.** `ferric mcp` and `fetch_reference` are built and
> tested today. The in-tree `fetch_reference` backend (recursive read + heading
> chunk + keyword score) is a deliberately simple stand-in that honors the *same
> contract* a future **Animus Dark Matter MCP knowledge server** will implement.
> Swapping the stand-in for the real Dark Matter server — a networked knowledge
> service behind that contract — is stated intent, tracked in the Dark Matter
> repo, not shipped here.

## Related

- [ICM — Agent Delegation](icm.md) — where `fetch_reference` lives.
- [Ornstein — Quarantined Research](ornstein.md) — the *untrusted* retrieval
  path, distinct from Dark Matter's trusted knowledge layer.

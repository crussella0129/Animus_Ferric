# Configuration

Ferric reads a small, **named** set of config inputs — never a generic key-value
map, and none of them can touch security policy (that stays hardcoded, ADR-005).
Everything is per-workspace and optional.

---

## Precedence

For the tunables below, Ferric resolves each value as:

**CLI flag > project `.ferric/config.toml` > user config > built-in default**

So config sets your defaults and flags override them per run.

---

## `.ferric/config.toml` — project defaults

A TOML file at `<workspace>/.ferric/config.toml`. Every field is optional; set
only what you want to stop repeating on the command line.

```toml
# Inference backend + model
backend     = "openai"          # currently the one backend
model       = "qwen2.5-coder:7b"
api_base    = "http://localhost:8080/v1"   # omit to auto-discover `ferric server`
api_key     = ""                # if your server needs one

# Model profile -> run policy (ADR-006)
params_b    = 7.0               # parameter count in billions
quant       = "Q4_K_M"
family      = "qwen"
ctx         = 8192              # context window in tokens
temperature = 0.0              # 0.0 = deterministic sampler

# Behavior
max_ring    = 1                # cap the active tool ring (restrict-only)
profile_dir = "benchmarks"     # where model_profiles.json lives (ADR-029)
stream      = true             # stream output by default

[hooks]                        # see "Hooks" below
pre_turn  = "scripts/pre.sh"
post_turn = "scripts/post.sh"
on_error  = "scripts/err.sh"
```

The field list is fixed and bounded on purpose — config can configure behavior,
but it can never reach the guard, denylists, or workspace boundary.

---

## `Animus.md` — freeform instructions

A Markdown file at the **workspace root** (like a `CLAUDE.md`). Its content is read
verbatim (no parsing) and folded into the system prompt as a distinct block —
trusted, user-authored context. Use it for project conventions the agent should
always follow.

```markdown
# Animus.md
- Prefer small, surgical edits over full rewrites.
- Follow the existing error-handling style (thiserror).
- Never touch the vendored/ directory.
```

When present, a run logs `Animus.md applied (N chars)`. (On a `--resume` run the
system prompt is frozen from the replayed trace, so an edited `Animus.md` is inert
for that continuation — Ferric tells you when this happens.)

---

## `.ferricignore` — additive path denylist {#ferricignore}

A gitignore-flavored file at the **workspace root** listing paths the agent must
not touch, on top of the hardcoded guard. **Additive-only**: it can only *add*
denials, never relax the compile-time floor (ADR-068).

```gitignore
# .ferricignore
secrets/            # any component named "secrets", anywhere
*.pem               # any file with this basename glob
data/private        # this path (and anything under it) from the workspace root
node_modules/
```

Pattern kinds:

| Pattern | Matches |
|---|---|
| `secrets` / `secrets/` | any path *component* equal to it (the dir/file anywhere) |
| `*.pem` | a basename glob (simple `*` only) |
| `data/private` | that relative path, anchored at the workspace root, and anything beneath |

An ignored path is off-limits at **every** level (read/write/execute); denials show
up in the trace with rule `ferricignore`. Blank lines and `#` comments are skipped.
The file is user-authored (never the LLM), and `.ferricignore` is itself
write-protected so the agent can't disable its own restrictions.

---

## Hooks — scripts on loop boundaries {#hooks}

Run your own shell scripts at deterministic points in the agent loop, configured in
`.ferric/config.toml` under `[hooks]`:

| Hook | Fires | On failure |
|---|---|---|
| `pre_turn` | before each turn's tool dispatch | stops the loop (`HookFailed`) |
| `post_turn` | after each turn | logged, non-fatal |
| `on_error` | when the run ends in an error stop | logged, non-fatal |

```toml
[hooks]
pre_turn  = "cargo fmt --check"        # e.g. gate each turn on a clean format
post_turn = "echo turn-done >> hook.log"
on_error  = "notify-send 'ferric run failed'"
```

Hooks run via the host shell (`sh -c` / `cmd /C`) in the workspace root. They are a
per-workspace escape hatch you author — distinct from cron jobs, whose command is a
bounded enum, not arbitrary shell.

---

## Environment variables

| Variable | Effect |
|---|---|
| `FERRIC_LOG` | per-crate log filter, e.g. `ferric_loop=debug,ferric_tools=trace` (overrides `-v`) |
| `RUST_LOG` | same, honored if `FERRIC_LOG` is unset |
| `FERRIC_PROMPTS_DIR` | prompt-element library dir (like `--prompts-dir`) |

---

## Data & runtime files

Written under `.ferric/` in the workspace (all git-ignorable):

| Path | What |
|---|---|
| `.ferric/server.json` | running server runfile (auto-discovery) |
| `.ferric/trace/*.jsonl` | per-session traces (the source of truth) |
| `.ferric/cron/*.toml` + `.state.json` | cron jobs + last-run state |
| `.ferric/MEMORY.md` | dream-mode consolidated memory |
| `.ferric/tasks/` | background-task stdout/stderr redirects |

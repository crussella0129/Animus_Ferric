# Configuration

Ferric reads a small, **named** set of config inputs — never a generic key-value
map, and none of them can touch security policy (that stays hardcoded, ADR-005).
Everything is per-workspace and optional.

## Ordinary sessions

Start with `cargo r` in the repository, or installed `ferric` in a work folder.
Existing expert configuration supplies model/endpoint choices and applicable
defaults; setup does not rewrite `.ferric/config.toml`. A missing configuration
does not require a settings questionnaire.

With an installed `llama-server`, setup can select a regular GGUF already in
the workspace's `models` directory. Local launch defaults to CPU, zero GPU
layers, and a 4096-token context unless context is explicitly configured.
These settings are unqualified: model metadata does not establish memory fit,
speed, grammar support, or coding capability. No model/engine downloads or
capability benchmarks run implicitly.

After successful preparation, `.ferric/startup-preference.json` remembers the
selected model coordinate. It contains no API key or permission to edit files,
does not replace `model_profiles.json`, and is revalidated before reuse.
Malformed or stale preferences require a fresh selection or an explicit error.
The human session uses conservative tool limits even when expert tier/ring
overrides exist; expert commands retain their configured controls.
Trace provenance labels these limits `conservative`, not measured capability
or an expert override.

Ask-only gives the model no file tools. Folder work requires consent on each
session or `--allow-edits`, and grants no shell, hooks, or delegation. Ferric
still writes its own preferences and traces in both modes. Session traces use
the shared `.ferric/trace` directory.

`.ferric-startup.lock` serializes startup within one workspace and remains on
disk after the session; do not infer a running process from its presence or
remove it while a session may be active. Status and explain read configuration
and local resources without health probes, lock creation, or writes. They do
not represent completed or resumable application checkpoints.

Cancellation is bounded during startup and provider requests. In folder-work
mode, an existing Git snapshot may delay cancellation until Git returns;
T-12024 tracks that remaining boundary.

---

## Precedence

For the tunables below, Ferric resolves each value as:

**CLI flag > project `.ferric/config.toml` > user config > built-in default**

So config sets your defaults and flags override them per run.

Only an absent file uses defaults. An unreadable, malformed, oversized, or
invalid present configuration stops the command before provider requests,
hooks, or agent work. This applies even when a CLI flag would override the
invalid setting: a broken project layer must never reveal user-level hooks or
skills beneath it. Errors name the file or setting and a corrective action;
they do not print configuration contents, credentials, or parser excerpts.
Configuration files must be regular UTF-8 files of at most 1 MiB. Previously
tolerated unknown fields, including the retired `backend` key, remain tolerated.

Parameter count must be finite and greater than zero, context must be greater
than zero, temperature must be finite and between 0 and 2 inclusive, and a
configured tool ring must be between 0 and 3 inclusive. The same checks apply
to effective CLI settings. These limits validate inputs; they do not prove
that a model fits the host or has earned a capability tier.

Query, chat, MCP and API select configuration and managed backends from the
same `--workspace`; ICM uses its pipeline root. An explicit API endpoint still
wins over discovery. Conflicting or unverifiable managed registrations require
operator resolution. Relative expert paths such as `profile_dir` retain their
existing invocation-directory interpretation.

---

## `.ferric/config.toml` — project defaults

A TOML file at `<workspace>/.ferric/config.toml`. Every field is optional; set
only what you want to stop repeating on the command line.

```toml
# Model + inference server (the OpenAI-compatible HTTP valve is the only backend)
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
harness_policy = "legacy"      # legacy | evidence (evidence_planner is unavailable)
profile_dir = "benchmarks"     # where model_profiles.json lives (ADR-029)
stream      = true             # stream output by default

[hooks]                        # see "Hooks" below
pre_turn  = "scripts/pre.sh"
post_turn = "scripts/post.sh"
on_error  = "scripts/err.sh"
```

`stream = false` disables streaming in both query and chat, including chat's
`/do` turns. `--no-stream` also disables it. MCP and API retain their own
transport behavior. An omitted harness policy remains omitted until execution:
fresh runs use Legacy and resumed runs inherit their trace's policy.

The expert API continues to reload valid configuration for each request;
MCP and chat hold their admitted settings for the session. An invalid API
configuration is rejected at startup or, if changed later, at the next request.
`quant` and `family` remain accepted compatibility labels; ordinary query/chat
policy selection does not use them to infer model capability.

The field list is fixed and bounded on purpose — config can configure behavior,
but it can never reach the guard, denylists, or workspace boundary.

`--trace-dir` is deliberately not a config field. It is an explicit,
query-only filesystem boundary. Omitting it uses
`<workspace>/.ferric/trace`; a relative value is resolved from the invocation
directory. An external value must be disjoint from the canonical workspace and
must not resolve through a symbolic link or Windows reparse point. Resume from
an external source trace requires the operator to repeat the same
`--trace-dir` and `--workspace` explicitly. Every supported incomplete,
resumable query stop prints a command targeting PowerShell on Windows or POSIX
`sh` on Unix, not `cmd.exe`; clarification alone adds the `--answer` argument.
Successful terminal traces are rejected by `--resume`, but an incomplete
resumable `session_end` remains valid. This low-level control is not a
high-level run/resume/evidence workflow.

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

Query and ICM use configured hooks. Chat, MCP, and API do not implicitly run
them. Query and chat honor the standing `allowed_skills` list; MCP, API, and ICM
retain their separate skill-authorization rules.

---

## Environment variables

| Variable | Effect |
|---|---|
| `FERRIC_LOG` | per-crate log filter, e.g. `ferric_loop=debug,ferric_tools=trace` (overrides `-v`) |
| `RUST_LOG` | same, honored if `FERRIC_LOG` is unset |
| `FERRIC_PROMPTS_DIR` | prompt-element library dir (like `--prompts-dir`) |

---

## Data & runtime files

Written under `.ferric/` in the workspace (all git-ignorable), apart from the
workspace-root startup lock and an individual `ferric query --trace-dir <DIR>`
writing that query's trace to its validated external directory:

| Path | What |
|---|---|
| `.ferric/server.json` | running server runfile (auto-discovery) |
| `.ferric/trace/*.jsonl` | default per-session traces (the source of truth); query traces may use explicit `--trace-dir` |
| `.ferric/startup-preference.json` | atomically saved model choice; separate from expert configuration and capability profiles |
| `.ferric-startup.lock` | persistent, ignored workspace coordination file; the operating-system lock is released after owned cleanup |
| `.ferric/cron/*.toml` + `.state.json` | cron jobs + last-run state |
| `.ferric/MEMORY.md` | dream-mode consolidated memory |
| `.ferric/tasks/` | background-task stdout/stderr redirects |

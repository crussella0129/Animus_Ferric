# Sprint 106 research — the environment-variable surface

**Goal (user):** "a thorough scrub for raw env variables that could be
manipulated if found."

## The complete inventory

Every runtime environment read in the workspace. There are only six, which is
the good news; the interesting part is what each one *reaches*.

| var | read at | what it controls | reachable consequence |
|---|---|---|---|
| `XDG_CONFIG_HOME` | `config.rs:87` | user config path | **arbitrary command execution** — see below |
| `APPDATA` | `config.rs:84` | user config path (Windows) | same |
| `HOME` | `config.rs:90` | user config path (fallback) | same |
| `OPENAI_API_KEY` | `backend.rs:65` | provider credential | credential |
| `FERRIC_PROMPTS_DIR` | `query.rs:338` | system-prompt library dir | the agent's system prompt |
| `FERRIC_LOG` / `RUST_LOG` | `logging.rs:49-52` | log verbosity | verbosity only |
| `TMPDIR`/`TEMP` (via `env::temp_dir()`) | `trace_verify.rs:186` | temp file location | predictable filename, below |

`env!("CARGO_PKG_VERSION")` / `env!("CARGO_MANIFEST_DIR")` are **compile-time**
and not runtime-manipulable; the only `CARGO_MANIFEST_DIR` use is inside
`#[cfg(test)]`, so no shipped binary carries a build-machine path.

## Finding 1 — the env→config→hooks chain is arbitrary code execution

The sharpest result, and the direct answer to the question asked:

```
XDG_CONFIG_HOME (or APPDATA, or HOME)
  -> <that>/ferric/config.toml
  -> Config.hooks                       (config.rs:27)
  -> run_hook()                         (hooks_exec.rs:6)
  -> sh -c <string>  /  cmd.exe /C <string>
```

`run_hook` passes a config-supplied string to a shell, in the workspace, with
the **full inherited environment** — including `OPENAI_API_KEY`. So anyone who
can set one environment variable for a Ferric process can run arbitrary
commands as that user and read the credential.

**Whether that is a *vulnerability* depends on the boundary being crossed, and
being honest about that matters more than the finding itself.** On a normal
desktop, setting another process's env already implies code execution as that
user, so this grants nothing new. It becomes real where env crosses a trust
boundary that code execution does not: CI runners taking env from a
pull-request-controlled file, container orchestration, or any wrapper script
that forwards a caller-supplied environment.

Two mitigations already hold and should be stated rather than re-derived:

- The **model cannot reach any of this.** `.ferric` is in *both*
  `DENIED_WRITE_SEGMENTS` and `DENIED_READ_SEGMENTS`, so the project config is
  unreadable and unwritable by the agent (ADR-092), and the user config lives
  outside the workspace, so containment excludes it.
- Hooks are user-installed, which is the same consent model settled for skills
  in ADR-091.

**So the gap is not authority, it is disclosure.** Nothing tells the user
*which* config file supplied a hook. A hook arriving from an unexpected
`XDG_CONFIG_HOME` looks exactly like one the user wrote.

## Finding 2 — `--api-key` is a command-line flag

`backend.rs:38`. Anything passed there is visible in `ps`/Task Manager to other
users on the machine and lands in shell history. The env var and the config
file are both safer, and both already work.

Good, and verified rather than assumed: the key reaches only an
`Authorization: Bearer` header (`openai.rs:209,336`) — never argv of a spawned
process, never a trace event.

**CORRECTION — this report was wrong on first writing.** It claimed
`OpenAiConfig`, `BackendOpts` *and* `Config` all lack a `Debug` impl. That was
read off a `grep -B4` whose context window stopped one line short of
`config.rs:12`. **`Config` does derive `Debug`, and holds `api_key` in
plaintext** — so the credential was one `{:?}` away from any log line,
`assert_eq!` failure message, or panic payload. Nothing prints one today, which
is a property of the current call sites and not of the type.

The error was caught by writing the check as a test with a positive control
instead of trusting the grep. That is the whole argument for testing an absence
rather than asserting it: the grep and the compiler disagreed, and the compiler
was right.

## Finding 3 — a predictable temp filename

`trace_verify.rs:186` writes `env::temp_dir().join("verify.jsonl")` — a fixed
name in a world-writable shared directory, deleted-then-recreated. On a
multi-user host another user can pre-create or symlink that path. Low
severity (it is a verification scratch file), trivially fixed with a unique
name.

## Finding 4 — `FERRIC_PROMPTS_DIR` redirects the system prompt

`query.rs:338`. Env-set, no validation, and it decides which prompt library
composes the agent's system prompt. Same boundary discussion as finding 1 — and
the same fix: **say where the system prompt came from**, since a silently
redirected prompt library is indistinguishable from the default.

## What the scan did *not* find

- **No bearer credentials anywhere.** `tskey-auth`/`tskey-api`, `ghp_`,
  `github_pat_`, `sk-…`, `AKIA…` and PEM private keys: zero hits in the tree
  **and zero across all history** (`git log --all -p`). This is the class that
  matters most, because a Tailscale auth key or API key is a *bearer* token —
  it adds a node or calls the API with no SSO and no hardware key, bypassing a
  security key entirely. None present.
- No `env::set_var` anywhere. The only child-env manipulation is
  `GIT_INDEX_FILE` (ADR-073's index protection) and the `llama-server` launch
  env, both harness-constructed, neither user- nor model-supplied.

## Scope

Fix findings 2 (test the absence), 3 (unique name), and the disclosure half of
1 and 4. Do **not** restrict config discovery — XDG is the correct convention
and breaking it would trade a real feature for a boundary the OS already owns.

# Environment variables

Every environment variable Ferric reads at runtime, what it reaches, and what
that means if someone else controls it. Audited end-to-end in sprint 106
(ADR-097).

There are six. The short version: none of them are secrets Ferric invents, and
the model can reach none of them — but two select a *file* whose contents
Ferric then trusts, and one of those files can carry shell commands.

| variable | read at | selects | worst consequence if attacker-set |
|---|---|---|---|
| `XDG_CONFIG_HOME` | `config.rs` | user config path | **arbitrary command execution** (via `hooks`) |
| `APPDATA` | `config.rs` | user config path (Windows) | same |
| `HOME` | `config.rs` | user config path (fallback) | same |
| `FERRIC_PROMPTS_DIR` | `query.rs` | prompt library dir | replaces the agent's system prompt |
| `OPENAI_API_KEY` | `backend.rs` | provider credential | supplies a credential |
| `FERRIC_LOG` / `RUST_LOG` | `logging.rs` | log verbosity | verbosity only |

`TMPDIR`/`TEMP` are read indirectly by `std::env::temp_dir()` in
`ferric trace verify`, which writes a scratch trace there.

## The one chain worth understanding

```
XDG_CONFIG_HOME  ->  <that>/ferric/config.toml  ->  [hooks]  ->  sh -c / cmd /C
```

`hooks` is the only config field that becomes **arbitrary command execution**:
`run_hook` hands the configured string to a shell, in the workspace, with the
full inherited environment — which includes `OPENAI_API_KEY`. The user config's
*location* is chosen by environment variable, so in principle one env var
selects a file that runs commands.

**This is not treated as a privilege boundary, deliberately.** On a normal
desktop, being able to set another process's environment already implies being
able to run code as that user, so locking it down would trade away the XDG
convention for a boundary the operating system already owns. It matters where
environment crosses a trust boundary that code execution does not — a CI runner
taking env from a pull-request-controlled file, or a container manifest.

What Ferric does instead is **disclose**. When hooks are in effect it prints:

```
hooks: loaded from /home/you/.config/ferric/config.toml
```

A hook you wrote and a hook that arrived from a config you did not know was
being read used to look identical. Now they do not.

**If you run Ferric anywhere the environment is not fully yours**, pass
`--workspace` explicitly and check that line, or set `XDG_CONFIG_HOME` yourself
so the user layer resolves somewhere you control.

## What the model can reach: nothing here

- `.ferric/` is in **both** `DENIED_WRITE_SEGMENTS` and `DENIED_READ_SEGMENTS`,
  so the agent can neither read nor write the project config — verified live
  against a real model (ADR-092).
- The user config lives outside the workspace, so guard containment excludes
  it.
- There is no `env::set_var` anywhere in the workspace, and a child process
  cannot alter its parent's environment. The only child env Ferric constructs
  is `GIT_INDEX_FILE` (ADR-073) and the `llama-server` launch env — both
  harness-built, neither user- nor model-supplied.

## Credentials

`OPENAI_API_KEY` is read from the environment; `--api-key` and
`api_key` in `config.toml` also work.

**Prefer the environment variable or the config file over `--api-key`.**
Anything on a command line is visible to other users via `ps`/Task Manager and
lands in shell history.

The key reaches exactly one place — an `Authorization: Bearer` header. It is
never written to a trace, never passed in the argv of a spawned process, and
cannot be printed:

- `Config` implements `Debug` **by hand**, rendering `api_key` as
  `<redacted>` while keeping presence visible for debugging config precedence.
  It previously derived `Debug`, which put the key one `{:?}` from a log.
- `BackendOpts` and `OpenAiConfig` implement no `Debug` at all, and a
  **compile-time** assertion in each crate fails the build if someone adds one,
  naming the redacting alternative.

The project config that may hold the key, `.ferric/config.toml`, is gitignored
and unreadable by the model.

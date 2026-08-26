Finalized - DO NOT EDIT

# Sprint 38 Build Plan

## Schema Tree
- Sprint Goal: persistent configuration + `Animus.md`
  - Config foundation
    - T-3801: `Config` struct + layered load (testable diagnostics, injectable path resolution)
  - CLI wiring — `ferric query`
    - T-3802: mechanical clap-default removal (behavior-preserving)
    - T-3803: config loading + precedence resolution + `model_key` fix
  - CLI wiring — `ferric mcp`
    - T-3804: mechanical clap-default removal (behavior-preserving)
    - T-3805: config loading + precedence resolution + `model_key` fix
  - Project instructions
    - T-3806: `Animus.md` — read + fold into the system prompt
  - Docs
    - T-3807: ADR-048 + docs

## Execution Sequence

### T-3801: `Config` struct + layered load
- **Touches:** `crates/ferric-cli/src/config.rs` (new), `crates/ferric-cli/src/backend.rs`
  (`BackendArg` gains `Serialize`/`Deserialize`, `#[serde(rename_all = "kebab-case")]`)
- **Depends on:** (none)
- `Config` struct (serde `Deserialize`, every field `Option<T>`): `backend`, `model_dir`,
  `model_file`, `model`, `api_base`, `api_key`, `params_b`, `quant`, `family`, `ctx`, `temperature`,
  `max_ring`, `profile_dir`, `stream`.
- `project_config_path(workspace: &Path) -> PathBuf` — `<workspace>/.ferric/config.toml` (mirrors
  `server.rs`'s `runfile_path`).
- **(C-003)** `user_config_path_from(env: &impl Fn(&str) -> Option<String>) -> Option<PathBuf>` —
  the test-injectable core: takes a lookup closure instead of touching real process env directly,
  so each branch is independently unit-testable. Branches, in order: Windows (`APPDATA` set →
  `<APPDATA>/ferric/config.toml`); XDG (`XDG_CONFIG_HOME` set → `<XDG_CONFIG_HOME>/ferric/config.toml`);
  HOME-fallback (`HOME` set, neither above → `<HOME>/.config/ferric/config.toml`); else `None`.
  `user_config_path() -> Option<PathBuf>` is a one-line wrapper calling
  `user_config_path_from(&|k| std::env::var(k).ok())`.
- **(C-004)** Diagnostics are testable data, not a bare `eprintln!` — mirrors the existing
  `prompt_composition_error: Option<String>` pattern already carried on `RunConfig`.
  `pub struct LoadedConfig { pub config: Config, pub diagnostics: Vec<String> }`.
  `load_layered_from(project_path: &Path, user_path: Option<&Path>) -> LoadedConfig` — the
  test-injectable merge core (project field wins over user field wins over `None`); a malformed
  TOML file at either path pushes one human-readable diagnostic string (naming the path and the
  parse error) into `diagnostics` and treats that layer as absent, rather than panicking.
  `load_layered(workspace: &Path) -> LoadedConfig` — the real entry point, resolving both real
  paths and calling the above. Callers (T-3803/T-3805) both `eprintln!` each diagnostic AND thread
  them into `RunConfig` for Note-tracing (matching `prompt_composition_error`'s existing dual
  handling), so the malformed-layer case is provably testable AND visibly reported.
- **Success criterion (EARS):**
  - **WHEN** `.ferric/config.toml` exists and parses, **THEN** `load_layered_from` **SHALL**
    populate `LoadedConfig.config` with exactly its set fields and an empty `diagnostics`.
  - **WHEN** both a project and a user config set the SAME field, **THEN** the project's value
    **SHALL** win.
  - **WHEN** neither file exists, **THEN** `load_layered_from` **SHALL** return an all-`None`
    `Config` and an empty `diagnostics` (silent no-op, no error).
  - **WHEN** a config file contains malformed TOML, **THEN** `load_layered_from` **SHALL** treat
    that layer as absent AND push one diagnostic string into `LoadedConfig.diagnostics` (never
    panic or abort the CLI).
  - **WHEN** `user_config_path_from` is given an env lookup where only `APPDATA` resolves,
    **THEN** it **SHALL** return the Windows-shaped path; only `XDG_CONFIG_HOME` resolves, **THEN**
    the XDG-shaped path; only `HOME` resolves, **THEN** the `.config`-fallback path; nothing
    resolves, **THEN** `None`.

### T-3802: Mechanical clap-default removal for `ferric query` (behavior-preserving)
- **Touches:** `crates/ferric-cli/src/query.rs`
- **Depends on:** (none — pure type-signature change, no config awareness yet)
- `params_b`/`quant`/`family`/`ctx`/`temperature`/`profile_dir` on `QueryArgs` lose their clap
  `default_value_t`/`default_value`, becoming bare `Option<T>`. Every existing call site that built
  `RunConfigArgs` from these fields is updated to apply the SAME hardcoded default values it uses
  today via `.unwrap_or(today's constant)`, with no config involved yet. This task is a pure
  refactor: isolates the mechanical clap-type change from the config-precedence logic (C-002), so a
  behavior regression is attributable to exactly one of the two tasks.
- **Success criterion (EARS):**
  - **WHEN** no CLI flag is passed for any of the six fields, **THEN** `run_query`'s resolved
    values **SHALL** be byte-identical to today's clap defaults (regression, provable in isolation
    from any config file).

### T-3803: Config loading + precedence resolution for `ferric query`
- **Touches:** `crates/ferric-cli/src/query.rs`
- **Depends on:** T-3801, T-3802
- `run_query` calls `Config::load_layered(&workspace_root)` once, then resolves each relevant field
  (the six from T-3802, plus `BackendOpts`' already-optional fields, plus `max_ring`/`stream`) as
  `cli_arg.or(config.field).unwrap_or(todays_hardcoded_default)` when building `RunConfigArgs`.
  Diagnostics from `LoadedConfig` are `eprintln!`'d and threaded into `RunConfig` for Note-tracing.
  **(C-001)** `model_key` — currently derived as
  `args.backend_opts.model.clone().or_else(|| args.backend_opts.model_file.clone())`, feeding
  `ferric_bench::read_profile`'s ADR-029 profile lookup — **MUST be re-derived from the
  post-merge, config-resolved `model`/`model_file` values**, not the raw `args.backend_opts` fields.
  Concretely: compute the resolved `model`/`model_file` first (via the `.or(config.field)` chain
  above), THEN derive `model_key` from those resolved values. Getting this wrong means a
  config-only-set `model` silently skips profile lookup and loses its earned
  `measured_level`/`calibrated_ring` promotion with no error or trace.
- **Success criterion (EARS):**
  - **WHEN** no config file and no relevant CLI flag are present, **THEN** behavior **SHALL** be
    byte-identical to today's hardcoded defaults.
  - **WHEN** a config file sets a field and no CLI flag overrides it, **THEN** the config value
    **SHALL** be used.
  - **WHEN** both a CLI flag and a config value are present for the same field, **THEN** the CLI
    flag **SHALL** win.
  - **WHEN** `model` (or `model_file`) is set ONLY via config (no matching CLI flag), **THEN**
    `model_key` **SHALL** still be derived from that config-resolved value, and the ADR-029 profile
    lookup **SHALL** still be attempted.
  - **WHEN** a config layer is malformed, **THEN** its diagnostic **SHALL** be traced as a `Note`
    (in addition to the stderr print) rather than only appearing on stderr.

### T-3804: Mechanical clap-default removal for `ferric mcp` (behavior-preserving)
- **Touches:** `crates/ferric-cli/src/mcp.rs`
- **Depends on:** (none)
- Same shape as T-3802, applied to whichever `McpArgs` fields mirror `QueryArgs`' six
  (`McpArgs` excludes `prompt`/`files` per ADR-046, otherwise the same set).
- **Success criterion (EARS):** identical shape to T-3802's clause, scoped to `ferric mcp`.

### T-3805: Config loading + precedence resolution for `ferric mcp`
- **Touches:** `crates/ferric-cli/src/mcp.rs`
- **Depends on:** T-3801, T-3804
- Same shape as T-3803 (config load, `.or(config.field).unwrap_or(default)` resolution,
  diagnostics threaded to stderr + `Note`), including the **same C-001 `model_key` fix** applied to
  `McpServer::launch`'s own profile-lookup call site.
- **Success criterion (EARS):** identical five clauses to T-3803's, scoped to `ferric mcp`.

### T-3806: `Animus.md` — read + fold into the system prompt
- **Touches:** `crates/ferric-cli/src/query.rs`, `crates/ferric-cli/src/mcp.rs`
- **Depends on:** (none)
- Reads `<workspace>/Animus.md` if present (plain `std::fs::read_to_string`, no parsing). When
  present, appended to whichever system prompt text is already produced (the oovra-composed prompt
  or `DEFAULT_SYSTEM_PROMPT`) as a distinct, clearly-delimited block. Absence is a silent no-op —
  **(C-005)** matching the existing `prompt_composition_error`-style precedent of tracing only the
  notable case, presence is traced as a `Note`; absence is NOT traced (the ordinary default path
  stays silent, consistent with how "no `prompts_dir` configured" is already untraced today).
- **Success criterion (EARS):**
  - **WHEN** `Animus.md` exists at the workspace root, **THEN** its content **SHALL** be appended
    to the system prompt as a distinct block, and a `Note` **SHALL** be traced recording that it
    was applied.
  - **WHEN** `Animus.md` is absent, **THEN** behavior **SHALL** be unchanged from today (no prompt
    change, no `Note`).

### T-3807: ADR-048 + docs
- **Touches:** `decisions.md`, `README.md`, `agent-tasks/agent-tasks.md`, `agent-tasks/completed-tasks.md`
- **Depends on:** T-3801, T-3802, T-3803, T-3804, T-3805, T-3806
- ADR-048: the config-precedence design and the bounded-field ADR-005 rationale; the `Animus.md`
  trust-tier decision (harness-owned context, not Ornstein-quarantined); the hand-rolled-vs-
  dependency call for the user-config path; **(C-006)** one confirming sentence noting
  `CompletionRequest::validate()`/`select_protocol` operate on final resolved values regardless of
  whether they originated from a CLI flag or a config file, so ADR-010's constraint/native-tools
  mutual exclusion is unaffected by config; explicit deferrals.
- **Success criterion (EARS):**
  - **WHEN** ADR-048 is read, **THEN** it **SHALL** state the precedence order, the bounded-field
    rationale, the `Animus.md` trust-tier decision, the ADR-010 non-interaction note, and explicitly
    list what's deferred.

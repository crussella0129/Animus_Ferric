//! Layered persistent configuration (ADR-048): `ferric query`/`ferric mcp`
//! resolve each tunable as CLI flag > project `.ferric/config.toml` > user
//! config > today's hardcoded default. A bounded, named field list — never a
//! generic key-value map — so "config never touches security/guard/denylist
//! policy" (ADR-005) is a structural fact, not a review-time hope.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::backend::BackendOpts;

#[derive(Clone, Default, Deserialize, PartialEq)]
pub struct Config {
    pub model: Option<String>,
    pub api_base: Option<String>,
    pub api_key: Option<String>,
    pub params_b: Option<f32>,
    pub quant: Option<String>,
    pub family: Option<String>,
    pub ctx: Option<u32>,
    pub temperature: Option<f32>,
    pub max_ring: Option<u8>,
    /// Persistent operator tier override (ADR-098) — the config spelling of
    /// `--tier`. Separate from `params_b`, which stays a fact about the model.
    pub tier: Option<crate::query::TierArg>,
    pub profile_dir: Option<PathBuf>,
    pub stream: Option<bool>,
    pub hooks: Option<ferric_core::HooksConfig>,
    /// Skills the user has standing-authorized for this workspace.
    ///
    /// This lives in config **because config lives under `.ferric/`**, which
    /// `ferric-guard` write-denies to the model. An allowlist the agent could
    /// edit would authorize nothing (sprint 100, ADR-091).
    ///
    /// Project-over-user like every other field, but note what that means here:
    /// a project allowlist *replaces* the user's rather than extending it, so a
    /// repo cannot silently inherit skills the user enabled globally.
    pub allowed_skills: Option<Vec<String>>,
}

/// Hand-written, not derived, so `api_key` can never be printed (ADR-097).
///
/// `Config` carries a credential in plaintext and previously derived `Debug`,
/// which put the key one `{:?}` away from a log line, an `assert_eq!` failure
/// message, or a panic payload. Nothing printed one *yet* — the derive was a
/// loaded gun rather than a fired one — and "nothing prints it today" is a
/// property of the current call sites, not of the type.
///
/// `Debug` is kept rather than removed because `assert_eq!` needs it, and a
/// type that cannot be compared in a test failure message is its own problem.
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("model", &self.model)
            .field("api_base", &self.api_base)
            // Presence is useful for debugging config precedence; the value
            // never is.
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("params_b", &self.params_b)
            .field("quant", &self.quant)
            .field("family", &self.family)
            .field("ctx", &self.ctx)
            .field("temperature", &self.temperature)
            .field("max_ring", &self.max_ring)
            .field("tier", &self.tier)
            .field("profile_dir", &self.profile_dir)
            .field("stream", &self.stream)
            .field("hooks", &self.hooks)
            .field("allowed_skills", &self.allowed_skills)
            .finish()
    }
}

impl Config {
    /// Project's value wins over user's, field-by-field.
    fn merged_over(self, user: Config) -> Config {
        Config {
            model: self.model.or(user.model),
            api_base: self.api_base.or(user.api_base),
            api_key: self.api_key.or(user.api_key),
            params_b: self.params_b.or(user.params_b),
            quant: self.quant.or(user.quant),
            family: self.family.or(user.family),
            ctx: self.ctx.or(user.ctx),
            temperature: self.temperature.or(user.temperature),
            max_ring: self.max_ring.or(user.max_ring),
            tier: self.tier.or(user.tier),
            profile_dir: self.profile_dir.or(user.profile_dir),
            stream: self.stream.or(user.stream),
            hooks: self.hooks.or(user.hooks),
            allowed_skills: self.allowed_skills.or(user.allowed_skills),
        }
    }
}

/// The result of a layered load: the merged config plus any diagnostics from
/// malformed layers — testable data (C-004), not a bare `eprintln!`. Mirrors
/// `RunConfig::prompt_composition_error`'s existing pattern of surfacing a
/// degrade-gracefully failure as data the caller traces once a sink exists.
pub struct LoadedConfig {
    pub config: Config,
    pub diagnostics: Vec<String>,
    /// Which config file supplied `hooks`, if any (ADR-097).
    ///
    /// Hooks are the one config field that becomes **arbitrary command
    /// execution**: `run_hook` hands the string to `sh -c` / `cmd /C` with the
    /// full inherited environment. And the *user* layer's location is chosen by
    /// environment variable (`XDG_CONFIG_HOME`, `APPDATA`, `HOME`), so the file
    /// that supplies it is env-selected.
    ///
    /// That is not treated as a privilege boundary — setting a process's
    /// environment normally already implies running code as that user, and the
    /// XDG convention is worth keeping. What was missing is that a hook from an
    /// unexpected config file looked exactly like one the user wrote. This
    /// makes the source nameable so a caller can disclose it.
    pub hooks_source: Option<PathBuf>,
}

/// `<workspace>/.ferric/config.toml` — mirrors `server.rs`'s `runfile_path`
/// convention (the `.ferric/` dir is already write-denied to the LLM, ADR-005).
pub fn project_config_path(workspace: &Path) -> PathBuf {
    workspace.join(".ferric").join("config.toml")
}

/// The test-injectable core (C-003): takes a lookup closure instead of
/// touching real process env directly, so each branch is independently
/// unit-testable without mutating real env vars. Checked in order: Windows
/// (`APPDATA`), XDG (`XDG_CONFIG_HOME`), then a `.config` HOME-fallback
/// (Linux/macOS without XDG set).
pub fn user_config_path_from(env: &impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if let Some(appdata) = env("APPDATA") {
        return Some(PathBuf::from(appdata).join("ferric").join("config.toml"));
    }
    if let Some(xdg) = env("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg).join("ferric").join("config.toml"));
    }
    if let Some(home) = env("HOME") {
        return Some(
            PathBuf::from(home)
                .join(".config")
                .join("ferric")
                .join("config.toml"),
        );
    }
    None
}

/// The real entry point: delegates to `user_config_path_from` against actual
/// process env.
pub fn user_config_path() -> Option<PathBuf> {
    user_config_path_from(&|k| std::env::var(k).ok())
}

/// Read + parse one layer. Absence is silent (`Config::default()`, no
/// diagnostic) — only a present-but-malformed file pushes one.
fn read_layer(path: &Path, diagnostics: &mut Vec<String>) -> Config {
    match std::fs::read_to_string(path) {
        Ok(text) => match toml::from_str::<Config>(&text) {
            Ok(config) => config,
            Err(e) => {
                diagnostics.push(format!(
                    "{}: malformed config, ignoring this layer: {e}",
                    path.display()
                ));
                Config::default()
            }
        },
        Err(_) => Config::default(),
    }
}

/// The test-injectable merge core: reads + parses each path if present,
/// merging project over user over `None`.
pub fn load_layered_from(project_path: &Path, user_path: Option<&Path>) -> LoadedConfig {
    let mut diagnostics = Vec::new();
    let project = read_layer(project_path, &mut diagnostics);
    let user = user_path
        .map(|p| read_layer(p, &mut diagnostics))
        .unwrap_or_default();
    // Resolved with the same project-wins rule the merge uses, so the reported
    // source is the file whose hooks actually take effect — not merely a file
    // that happened to mention them.
    let hooks_source = if project.hooks.is_some() {
        Some(project_path.to_path_buf())
    } else if user.hooks.is_some() {
        user_path.map(Path::to_path_buf)
    } else {
        None
    };
    LoadedConfig {
        config: project.merged_over(user),
        diagnostics,
        hooks_source,
    }
}

/// The real entry point: resolves both real paths and loads them.
pub fn load_layered(workspace: &Path) -> LoadedConfig {
    load_layered_from(
        &project_config_path(workspace),
        user_config_path().as_deref(),
    )
}

/// Merge CLI-supplied `BackendOpts` with config-resolved fallbacks: each
/// field keeps its CLI value if set, else falls back to `cfg`'s. Shared by
/// `run_query` (mutates `args.backend_opts` in place) and `McpServer::launch`
/// (mutates a local clone, since `McpArgs` is `&`) so the merge can't drift
/// between the two surfaces — and, per the test-critic's C-001/C-003, so it
/// has ONE call site that's directly unit-testable in isolation, rather than
/// being inlined at each of the two (previously duplicated) launch sites.
pub fn merge_backend_opts(mut opts: BackendOpts, cfg: &Config) -> BackendOpts {
    opts.model = opts.model.take().or_else(|| cfg.model.clone());
    opts.api_base = opts.api_base.take().or_else(|| cfg.api_base.clone());
    opts.api_key = opts.api_key.take().or_else(|| cfg.api_key.clone());
    opts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn load_layered_from_project_only() {
        let dir = tempfile::tempdir().unwrap();
        let project = write(
            dir.path(),
            "project.toml",
            "params_b = 8.0\nquant = \"Q8_0\"\n",
        );
        let loaded = load_layered_from(&project, None);
        assert!(loaded.diagnostics.is_empty());
        assert_eq!(loaded.config.params_b, Some(8.0));
        assert_eq!(loaded.config.quant, Some("Q8_0".to_string()));
        assert_eq!(loaded.config.family, None);
    }

    #[test]
    fn load_layered_from_user_only() {
        let dir = tempfile::tempdir().unwrap();
        let user = write(dir.path(), "user.toml", "temperature = 0.5\n");
        // A project path that doesn't exist — only the user layer applies.
        let loaded = load_layered_from(&dir.path().join("absent.toml"), Some(&user));
        assert!(loaded.diagnostics.is_empty());
        assert_eq!(loaded.config.temperature, Some(0.5));
    }

    #[test]
    fn load_layered_project_wins_on_overlap() {
        let dir = tempfile::tempdir().unwrap();
        let project = write(dir.path(), "project.toml", "quant = \"Q4_K_M\"\n");
        let user = write(dir.path(), "user.toml", "quant = \"Q8_0\"\n");
        let loaded = load_layered_from(&project, Some(&user));
        assert_eq!(loaded.config.quant, Some("Q4_K_M".to_string()));
    }

    #[test]
    fn load_layered_neither_present_is_all_none() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load_layered_from(
            &dir.path().join("no-project.toml"),
            Some(&dir.path().join("no-user.toml")),
        );
        assert!(loaded.diagnostics.is_empty());
        assert_eq!(loaded.config, Config::default());
    }

    #[test]
    fn load_layered_malformed_toml_pushes_diagnostic() {
        let dir = tempfile::tempdir().unwrap();
        let project = write(dir.path(), "project.toml", "this is not [valid toml");
        let user = write(dir.path(), "user.toml", "quant = \"Q8_0\"\n");
        let loaded = load_layered_from(&project, Some(&user));
        assert_eq!(loaded.diagnostics.len(), 1);
        assert!(loaded.diagnostics[0].contains("project.toml"));
        // The malformed layer degrades to None, but a valid layer beneath it
        // still applies.
        assert_eq!(loaded.config.quant, Some("Q8_0".to_string()));
    }

    #[test]
    fn project_config_path_is_workspace_relative() {
        let path = project_config_path(Path::new("/workspace"));
        assert_eq!(path, PathBuf::from("/workspace/.ferric/config.toml"));
    }

    fn env_with(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |k| {
            pairs
                .iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn user_config_path_from_appdata_only() {
        let env = env_with(&[("APPDATA", "C:/Users/x/AppData/Roaming")]);
        let path = user_config_path_from(&env).unwrap();
        assert_eq!(
            path,
            PathBuf::from("C:/Users/x/AppData/Roaming/ferric/config.toml")
        );
    }

    #[test]
    fn user_config_path_from_xdg_only() {
        let env = env_with(&[("XDG_CONFIG_HOME", "/home/x/.config")]);
        let path = user_config_path_from(&env).unwrap();
        assert_eq!(path, PathBuf::from("/home/x/.config/ferric/config.toml"));
    }

    #[test]
    fn user_config_path_from_home_fallback_only() {
        let env = env_with(&[("HOME", "/home/x")]);
        let path = user_config_path_from(&env).unwrap();
        assert_eq!(path, PathBuf::from("/home/x/.config/ferric/config.toml"));
    }

    #[test]
    fn user_config_path_from_nothing_resolves_to_none() {
        let env = |_: &str| None;
        assert_eq!(user_config_path_from(&env), None);
    }

    #[test]
    fn user_config_path_wrapper_uses_real_env() {
        // Shape check only — not asserting the exact real-machine value,
        // which varies by CI runner. Just confirm it delegates sensibly: if
        // it resolves at all, it must end in ferric/config.toml.
        if let Some(path) = user_config_path() {
            assert!(path.ends_with("ferric/config.toml") || path.ends_with("ferric\\config.toml"));
        }
    }

    /// Backward-compat: a legacy `backend = "openai"` key (written before the
    /// single-backend simplification removed the field) must still parse — the
    /// now-unknown key is ignored, not a hard error — so old
    /// `.ferric/config.toml` files keep working.
    #[test]
    fn legacy_backend_key_is_ignored_not_an_error() {
        let cfg: Config = toml::from_str("backend = \"openai\"\nmodel = \"m\"\n").unwrap();
        assert_eq!(cfg.model, Some("m".to_string()));
    }

    fn no_backend_opts() -> BackendOpts {
        BackendOpts {
            model: None,
            api_base: None,
            api_key: None,
        }
    }

    /// The `BackendOpts` fields get config-only
    /// precedence, in one pass — closing the rest of the C-001-class gap the
    /// critic flagged (not just `backend`).
    #[test]
    fn merge_backend_opts_config_only_remaining_fields_are_applied() {
        let cfg = Config {
            model: Some("mockmodel".to_string()),
            api_base: Some("http://example/v1".to_string()),
            api_key: Some("key123".to_string()),
            ..Config::default()
        };
        let merged = merge_backend_opts(no_backend_opts(), &cfg);
        assert_eq!(merged.model, Some("mockmodel".to_string()));
        assert_eq!(merged.api_base, Some("http://example/v1".to_string()));
        assert_eq!(merged.api_key, Some("key123".to_string()));
    }

    #[test]
    fn merge_backend_opts_cli_model_wins_over_config_model() {
        let cfg = Config {
            model: Some("config-model".to_string()),
            ..Config::default()
        };
        let mut opts = no_backend_opts();
        opts.model = Some("cli-model".to_string());
        let merged = merge_backend_opts(opts, &cfg);
        assert_eq!(merged.model, Some("cli-model".to_string()));
    }

    // --- ADR-097: the env-selected config layer that reaches a shell ---

    #[test]
    fn hooks_source_names_the_file_that_supplied_them() {
        let dir = tempfile::tempdir().unwrap();
        let user = write(
            dir.path(),
            "user.toml",
            "[hooks]\npre_turn = \"echo from-user\"\n",
        );
        let project = dir.path().join("absent.toml");

        let loaded = load_layered_from(&project, Some(&user));
        assert!(loaded.config.hooks.is_some());
        assert_eq!(
            loaded.hooks_source.as_deref(),
            Some(user.as_path()),
            "a hook that takes effect must be attributable to a file"
        );
    }

    /// The attribution has to follow the *merge*, not merely presence: the
    /// project layer wins, so the project file is the honest answer even when
    /// the user layer also defines hooks. Reporting the wrong file would be
    /// worse than reporting none.
    #[test]
    fn hooks_source_follows_the_project_wins_rule() {
        let dir = tempfile::tempdir().unwrap();
        let user = write(
            dir.path(),
            "user.toml",
            "[hooks]\npre_turn = \"echo from-user\"\n",
        );
        let project = write(
            dir.path(),
            "project.toml",
            "[hooks]\npre_turn = \"echo from-project\"\n",
        );

        let loaded = load_layered_from(&project, Some(&user));
        assert_eq!(loaded.hooks_source.as_deref(), Some(project.as_path()));
        assert_eq!(
            loaded.config.hooks.unwrap().pre_turn.as_deref(),
            Some("echo from-project"),
            "the reported source must be the one whose hook actually runs"
        );
    }

    #[test]
    fn no_hooks_means_no_source_to_disclose() {
        let dir = tempfile::tempdir().unwrap();
        let project = write(dir.path(), "project.toml", "model = \"m\"\n");
        let loaded = load_layered_from(&project, None);
        assert!(loaded.hooks_source.is_none());
    }

    /// `Config` carries a credential in plaintext and IS `Debug` (needed for
    /// `assert_eq!`), so the guarantee has to be about what `Debug` *prints* —
    /// not about the impl's absence. This asserts the property directly.
    #[test]
    fn debug_output_never_contains_the_api_key() {
        let cfg = Config {
            api_key: Some("sk-supersecret-do-not-print".to_string()),
            model: Some("some-model".to_string()),
            ..Config::default()
        };
        let rendered = format!("{cfg:?}");

        assert!(
            !rendered.contains("sk-supersecret-do-not-print"),
            "the api_key leaked into Debug output: {rendered}"
        );
        assert!(
            rendered.contains("<redacted>"),
            "presence of a key should still be visible for debugging config \
             precedence: {rendered}"
        );
        // A control: the redaction must be targeted, not a blanket blank impl
        // that hides everything and would pass the assertion above trivially.
        assert!(
            rendered.contains("some-model"),
            "non-secret fields must still be printed: {rendered}"
        );
    }

    /// A `Config` with no key must not claim to have a redacted one — the
    /// distinction is the whole point of keeping presence visible.
    #[test]
    fn debug_output_distinguishes_absent_from_redacted() {
        let rendered = format!("{:?}", Config::default());
        assert!(rendered.contains("api_key: None"), "{rendered}");
        assert!(!rendered.contains("<redacted>"), "{rendered}");
    }

    /// `BackendOpts` also holds `api_key` in plaintext and is NOT `Debug`
    /// today. That absence is load-bearing, and a later `#[derive(Debug)]`
    /// would erase it silently. Detection uses inherent-impl priority: an
    /// inherent associated const shadows the trait's, so `IS_DEBUG` is `true`
    /// only when the type really implements `Debug`.
    #[test]
    fn backend_opts_is_not_debug_printable() {
        struct IsDebug<T>(std::marker::PhantomData<T>);

        trait Fallback {
            const IS_DEBUG: bool = false;
        }
        impl<T> Fallback for IsDebug<T> {}

        impl<T: std::fmt::Debug> IsDebug<T> {
            const IS_DEBUG: bool = true;
        }

        // Compile-time, not runtime: adding `#[derive(Debug)]` to `BackendOpts`
        // should fail the BUILD, not merely a test someone might not run.
        //
        // The positive control comes first — without it, a detector that always
        // answered "not Debug" would satisfy the real check while verifying
        // nothing.
        const _: () = assert!(
            <IsDebug<String>>::IS_DEBUG,
            "the detector must recognise a type that IS Debug"
        );
        const _: () = assert!(
            !<IsDebug<BackendOpts>>::IS_DEBUG,
            "BackendOpts holds api_key in plaintext; if it must become Debug, \
             give it a redacting impl like Config's"
        );
    }
}

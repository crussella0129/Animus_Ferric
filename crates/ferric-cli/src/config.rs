//! Layered persistent configuration (ADR-048): `ferric query`/`ferric mcp`
//! resolve each tunable as CLI flag > project `.ferric/config.toml` > user
//! config > today's hardcoded default. A bounded, named field list — never a
//! generic key-value map — so "config never touches security/guard/denylist
//! policy" (ADR-005) is a structural fact, not a review-time hope.
//!
//! `#![allow(dead_code)]`: this module lands in T-3801, ahead of its call
//! sites in `query.rs`/`mcp.rs` (T-3803/T-3805) — deliberate task-attribution
//! granularity (C-002). Removed once those wire it in.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::backend::BackendArg;

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Config {
    pub backend: Option<BackendArg>,
    pub model_dir: Option<PathBuf>,
    pub model_file: Option<String>,
    pub model: Option<String>,
    pub api_base: Option<String>,
    pub api_key: Option<String>,
    pub params_b: Option<f32>,
    pub quant: Option<String>,
    pub family: Option<String>,
    pub ctx: Option<u32>,
    pub temperature: Option<f32>,
    pub max_ring: Option<u8>,
    pub profile_dir: Option<PathBuf>,
    pub stream: Option<bool>,
}

impl Config {
    /// Project's value wins over user's, field-by-field.
    fn merged_over(self, user: Config) -> Config {
        Config {
            backend: self.backend.or(user.backend),
            model_dir: self.model_dir.or(user.model_dir),
            model_file: self.model_file.or(user.model_file),
            model: self.model.or(user.model),
            api_base: self.api_base.or(user.api_base),
            api_key: self.api_key.or(user.api_key),
            params_b: self.params_b.or(user.params_b),
            quant: self.quant.or(user.quant),
            family: self.family.or(user.family),
            ctx: self.ctx.or(user.ctx),
            temperature: self.temperature.or(user.temperature),
            max_ring: self.max_ring.or(user.max_ring),
            profile_dir: self.profile_dir.or(user.profile_dir),
            stream: self.stream.or(user.stream),
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
    LoadedConfig {
        config: project.merged_over(user),
        diagnostics,
    }
}

/// The real entry point: resolves both real paths and loads them.
pub fn load_layered(workspace: &Path) -> LoadedConfig {
    load_layered_from(
        &project_config_path(workspace),
        user_config_path().as_deref(),
    )
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

    #[test]
    fn backend_arg_config_field_uses_kebab_case_spelling() {
        // Matches clap's own lowercase ValueEnum spelling ("mistral"/"openai").
        let cfg: Config = toml::from_str("backend = \"openai\"\n").unwrap();
        assert_eq!(cfg.backend, Some(BackendArg::Openai));
    }
}

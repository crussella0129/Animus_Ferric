//! Ferric's per-user data directory.
//!
//! `$FERRIC_DATA_DIR`, else `%LOCALAPPDATA%\ferric` on Windows, else
//! `$XDG_DATA_HOME/ferric` or `~/.local/share/ferric`. The engine and models
//! live here, outside every workspace, so one download serves every project.

use std::path::PathBuf;

/// Resolve the data directory from an environment lookup (injectable for
/// tests).
pub fn data_dir_from(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let get = |name: &str| env(name).filter(|value| !value.trim().is_empty());
    if let Some(dir) = get("FERRIC_DATA_DIR") {
        return Some(PathBuf::from(dir));
    }
    if cfg!(windows) {
        return get("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("ferric"));
    }
    get("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| get("HOME").map(|home| PathBuf::from(home).join(".local").join("share")))
        .map(|dir| dir.join("ferric"))
}

pub fn data_dir() -> Option<PathBuf> {
    data_dir_from(&|name| std::env::var(name).ok())
}

/// Where Ferric keeps the models it downloads.
pub fn managed_models_dir() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join("models"))
}

/// Where Ferric keeps the engines it installs.
pub fn engine_root() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join("engine"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        let pairs = pairs.to_vec();
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string())
        }
    }

    #[test]
    fn the_explicit_override_wins() {
        let found = data_dir_from(&env(&[("FERRIC_DATA_DIR", "/data/ferric"), ("HOME", "/h")]));
        assert_eq!(found, Some(PathBuf::from("/data/ferric")));
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_uses_xdg_then_home() {
        assert_eq!(
            data_dir_from(&env(&[("XDG_DATA_HOME", "/x"), ("HOME", "/h")])),
            Some(PathBuf::from("/x/ferric"))
        );
        assert_eq!(
            data_dir_from(&env(&[("HOME", "/h")])),
            Some(PathBuf::from("/h/.local/share/ferric"))
        );
        assert_eq!(data_dir_from(&env(&[("HOME", "  ")])), None);
    }
}

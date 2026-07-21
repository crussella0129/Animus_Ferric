//! `.ferricignore` — user-authored, **additive-only** path denials (ADR-068).
//!
//! A workspace may ship a `.ferricignore` file (gitignore-flavored) listing
//! paths the agent must not touch — `secrets/`, `*.pem`, vendored trees. It is
//! loaded from the workspace root and folded into the guard's decision at the
//! registry chokepoint.
//!
//! The invariant that keeps this consistent with ADR-005 ("security is
//! hardcoded, the LLM is never consulted"): `.ferricignore` can only **expand**
//! what is denied, never relax the compile-time floor. A pattern adds a denial;
//! nothing in the file can permit a path the hardcoded lists deny. The file is
//! user-authored (like `Animus.md`/`.ferric/config.toml`) — the model never
//! writes it (`.ferricignore` is itself write-denied so the agent cannot disable
//! its own restrictions).

use std::path::{Component, Path};

/// A parsed `.ferricignore` — an ordered set of additive denial rules.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IgnoreList {
    rules: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Rule {
    pattern: Pattern,
    /// The original source line, surfaced as the denial reason.
    source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Pattern {
    /// A bare name (no `/`, no `*`, e.g. `secrets` or `node_modules/`): matches
    /// any path *component* equal to it — the dir/file anywhere in the tree.
    Segment(String),
    /// A basename glob (contains `*`, no `/`, e.g. `*.pem`): matches a path's
    /// final component.
    Glob(String),
    /// A relative path (contains an internal `/`, e.g. `data/private`): matches
    /// that path or anything beneath it, anchored at the workspace root.
    PathPrefix(Vec<String>),
}

impl IgnoreList {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Parse `.ferricignore` text: one pattern per line; blank lines and `#`
    /// comments are skipped. A trailing `/` marks a directory (still matched as
    /// a component). Case-sensitive, matching gitignore/POSIX filesystems.
    pub fn parse(text: &str) -> Self {
        let mut rules = Vec::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Tolerate a trailing (directory) and/or leading slash for matching.
            let body = line.trim_matches('/');
            if body.is_empty() {
                continue;
            }
            let pattern = if body.contains('/') {
                Pattern::PathPrefix(body.split('/').map(str::to_string).collect())
            } else if body.contains('*') {
                Pattern::Glob(body.to_string())
            } else {
                Pattern::Segment(body.to_string())
            };
            rules.push(Rule {
                pattern,
                source: line.to_string(),
            });
        }
        IgnoreList { rules }
    }

    /// Load from `<root>/.ferricignore`; an absent or unreadable file yields an
    /// empty list (a pure no-op — the feature is opt-in per workspace).
    pub fn load(root: &Path) -> Self {
        std::fs::read_to_string(root.join(".ferricignore"))
            .map(|t| Self::parse(&t))
            .unwrap_or_default()
    }

    /// The source text of the first rule matching `resolved` (an absolute,
    /// guard-resolved path) taken relative to `root`, or `None` if none match.
    pub fn matches(&self, resolved: &Path, root: &Path) -> Option<String> {
        if self.rules.is_empty() {
            return None;
        }
        let rel = resolved.strip_prefix(root).unwrap_or(resolved);
        let components: Vec<String> = rel
            .components()
            .filter_map(|c| match c {
                Component::Normal(n) => n.to_str().map(str::to_string),
                _ => None,
            })
            .collect();
        let basename = components.last().cloned().unwrap_or_default();

        for rule in &self.rules {
            let hit = match &rule.pattern {
                Pattern::Segment(s) => components.iter().any(|c| c == s),
                Pattern::Glob(g) => glob_match(g, &basename),
                Pattern::PathPrefix(prefix) => starts_with_components(&components, prefix),
            };
            if hit {
                return Some(rule.source.clone());
            }
        }
        None
    }
}

/// Do `components` begin with `prefix` (component-wise)?
fn starts_with_components(components: &[String], prefix: &[String]) -> bool {
    prefix.len() <= components.len() && components.iter().zip(prefix).all(|(c, p)| c == p)
}

/// Match a simple `*`-glob (no `?`/`[]`) against `name`. `*` matches any run of
/// characters, including none. Multiple `*`s are supported.
fn glob_match(pattern: &str, name: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == name; // no wildcard: exact match
    }
    let mut pos = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            // Leading literal must be a prefix.
            if !name[pos..].starts_with(part) {
                return false;
            }
            pos += part.len();
        } else if i == parts.len() - 1 {
            // Trailing literal must be a suffix at or after the current position.
            return name[pos..].ends_with(part);
        } else {
            match name[pos..].find(part) {
                Some(found) => pos += found + part.len(),
                None => return false,
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn root() -> PathBuf {
        PathBuf::from("/ws")
    }

    #[test]
    fn parse_skips_comments_and_blanks() {
        let list = IgnoreList::parse("# a comment\n\nsecrets\n  \n*.pem\n");
        assert!(!list.is_empty());
        // 2 real rules (secrets, *.pem).
        assert_eq!(list.rules.len(), 2);
    }

    #[test]
    fn segment_matches_component_anywhere() {
        let list = IgnoreList::parse("secrets/\n");
        assert!(
            list.matches(&root().join("secrets/key.txt"), &root())
                .is_some()
        );
        assert!(
            list.matches(&root().join("a/b/secrets/x"), &root())
                .is_some()
        );
        assert!(list.matches(&root().join("src/main.rs"), &root()).is_none());
    }

    #[test]
    fn glob_matches_basename() {
        let list = IgnoreList::parse("*.pem\n");
        assert!(
            list.matches(&root().join("certs/server.pem"), &root())
                .is_some()
        );
        assert!(list.matches(&root().join("server.pem"), &root()).is_some());
        assert!(list.matches(&root().join("server.pub"), &root()).is_none());
        // A middle wildcard.
        let list2 = IgnoreList::parse("secret*.key\n");
        assert!(
            list2
                .matches(&root().join("secret_prod.key"), &root())
                .is_some()
        );
        assert!(list2.matches(&root().join("public.key"), &root()).is_none());
    }

    #[test]
    fn path_prefix_matches_from_root() {
        let list = IgnoreList::parse("data/private\n");
        assert!(
            list.matches(&root().join("data/private/db.sqlite"), &root())
                .is_some()
        );
        assert!(
            list.matches(&root().join("data/private"), &root())
                .is_some()
        );
        // Same trailing name but different location does NOT match a path prefix.
        assert!(
            list.matches(&root().join("other/private"), &root())
                .is_none()
        );
    }

    #[test]
    fn empty_list_matches_nothing() {
        let list = IgnoreList::empty();
        assert!(list.matches(&root().join("secrets/x"), &root()).is_none());
    }

    #[test]
    fn returns_the_source_line_as_reason() {
        let list = IgnoreList::parse("secrets/\n");
        assert_eq!(
            list.matches(&root().join("secrets/x"), &root()).as_deref(),
            Some("secrets/")
        );
    }
}

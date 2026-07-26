//! Agent skills in the conventional on-disk format: a directory per skill
//! holding a `SKILL.md` whose YAML frontmatter carries `name` and
//! `description`, and whose body is the instruction text.
//!
//! # The trust model
//!
//! A skill is **not** untrusted content in the way ingested web pages are
//! (ADR-081). The user chooses to install one; installation is the consent
//! event, and treating a deliberately-installed skill as contaminated would be
//! both wrong and unusable.
//!
//! But consent-to-install is not consent-to-run-whenever. Two properties keep
//! those separate:
//!
//! 1. **Only the user can install — and the model cannot even read what is
//!    installed.** Skills live under `.ferric/`, which is in *both* of
//!    `ferric-guard`'s denylists: `DENIED_WRITE_SEGMENTS` (so the model cannot
//!    author a skill or edit one it was given, nor edit the allowlist in
//!    `.ferric/config.toml`) and `DENIED_READ_SEGMENTS` (so it cannot open an
//!    unauthorized `SKILL.md` and follow it of its own accord).
//!
//!    The read half was a happy inheritance rather than a design: `.ferric` was
//!    read-denied in sprint 35 to keep the trace away from the model being
//!    traced. Skills landing there picked it up. Verified live (sprint 101):
//!    the 7B asked to read an unauthorized `SKILL.md` got
//!    `denied_read_segment matched .ferric`. **So the authorized prompt
//!    injection below is the only route from a skill to the model at all.**
//! 2. **Discovery is not authorization.** [`discover`] returns every skill on
//!    disk. Getting one into a prompt requires an [`Authority`] — an explicit
//!    user invocation or a user-written allowlist. **A model cannot authorize
//!    a skill for itself**, because nothing it can write is consulted.
//!
//! That is why [`DiscoveredSkill`] and [`AuthorizedSkill`] are different types
//! rather than one type with a boolean. The prompt composer accepts only the
//! latter, so "forgot to check the allowlist" is not a reachable state — the
//! same structural trick as the constrained-loop valve (ADR-010).

use std::path::{Path, PathBuf};

use thiserror::Error;

/// The file every skill directory must contain.
pub const SKILL_FILE: &str = "SKILL.md";

/// Where skills live, relative to a workspace root.
///
/// Under `.ferric/` deliberately: that segment is write-denied to the model
/// (`ferric-guard`), so installing a skill is something only the user can do.
pub fn skills_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".ferric").join("skills")
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SkillError {
    #[error("{0}: missing YAML frontmatter (a SKILL.md must open with a `---` line)")]
    NoFrontmatter(String),
    #[error("{0}: unterminated frontmatter (no closing `---`)")]
    UnterminatedFrontmatter(String),
    #[error("{0}: frontmatter is missing required key `{1}`")]
    MissingKey(String, &'static str),
    #[error("{0}: frontmatter key `{1}` is empty")]
    EmptyKey(String, &'static str),
    #[error(
        "{0}: declared name `{1}` does not match its directory `{2}` — a skill is addressed by \
         directory, so a mismatch would let one skill answer to another's name"
    )]
    NameMismatch(String, String, String),
    #[error("{0}: name must be non-empty, and use only lowercase letters, digits, `-` and `_`")]
    InvalidName(String),
    #[error("i/o error reading {0}: {1}")]
    Io(String, String),
}

/// A skill found on disk. Carries no permission to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredSkill {
    pub name: String,
    pub description: String,
    /// The instruction body (everything after the frontmatter), trimmed.
    pub instructions: String,
    pub path: PathBuf,
}

/// Why a skill is allowed to run this session.
///
/// There is no `Model` variant, and that absence is the design: every route to
/// running a skill traces back to a human decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Authority {
    /// Named directly on this invocation (`--skill <name>`), i.e. the user
    /// asked for it right now.
    UserRequested,
    /// Listed in the workspace config's skill allowlist, i.e. the user decided
    /// ahead of time. The config lives under `.ferric/` and is model-immutable.
    ConfigAllowed,
}

/// A skill cleared to run, and the reason it was cleared.
///
/// Constructible only via [`authorize`], so possessing one is proof the check
/// happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedSkill {
    skill: DiscoveredSkill,
    authority: Authority,
}

impl AuthorizedSkill {
    pub fn name(&self) -> &str {
        &self.skill.name
    }
    pub fn description(&self) -> &str {
        &self.skill.description
    }
    pub fn instructions(&self) -> &str {
        &self.skill.instructions
    }
    pub fn authority(&self) -> Authority {
        self.authority
    }
    pub fn path(&self) -> &Path {
        &self.skill.path
    }
}

/// Is `name` a legal skill name? Lowercase alphanumerics, `-`, `_`.
///
/// Restrictive because the name is used as a directory name and echoed into a
/// prompt: no path separators, no `..`, no whitespace, no control characters.
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Parse a `SKILL.md`. `dir_name` is the directory the file was found in, which
/// the declared `name` must match.
pub fn parse_skill(
    text: &str,
    dir_name: &str,
    path: PathBuf,
) -> Result<DiscoveredSkill, SkillError> {
    let label = path.display().to_string();

    // Frontmatter must be the very first thing in the file. A BOM is tolerated
    // because editors add it silently; anything else is a hard error rather
    // than a scan-forward, so a skill cannot hide a second frontmatter block
    // below prose.
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    let body = body
        .strip_prefix("---")
        .ok_or_else(|| SkillError::NoFrontmatter(label.clone()))?;
    let body = body
        .strip_prefix("\r\n")
        .or_else(|| body.strip_prefix('\n'))
        .ok_or_else(|| SkillError::NoFrontmatter(label.clone()))?;

    let (front, rest) = split_frontmatter(body)
        .ok_or_else(|| SkillError::UnterminatedFrontmatter(label.clone()))?;

    let name = front_value(front, "name").ok_or(SkillError::MissingKey(label.clone(), "name"))?;
    let description = front_value(front, "description")
        .ok_or(SkillError::MissingKey(label.clone(), "description"))?;

    if name.is_empty() {
        return Err(SkillError::EmptyKey(label, "name"));
    }
    if description.is_empty() {
        return Err(SkillError::EmptyKey(label, "description"));
    }
    if !is_valid_name(&name) {
        return Err(SkillError::InvalidName(label));
    }
    if name != dir_name {
        return Err(SkillError::NameMismatch(label, name, dir_name.to_string()));
    }

    Ok(DiscoveredSkill {
        name,
        description,
        instructions: rest.trim().to_string(),
        path,
    })
}

/// Split at the closing `---` line. Returns (frontmatter, remainder).
fn split_frontmatter(body: &str) -> Option<(&str, &str)> {
    let mut offset = 0usize;
    for line in body.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some((&body[..offset], &body[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

/// Read one `key: value` from the frontmatter.
///
/// Deliberately flat: no nesting, no lists, no anchors. A key we do not
/// understand is ignored rather than interpreted, and the two keys that matter
/// must be present as plain scalars.
fn front_value(front: &str, key: &str) -> Option<String> {
    for line in front.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        if k.trim() != key {
            continue;
        }
        let v = v.trim();
        // Tolerate the quoting styles a human will actually type.
        let v = v
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .or_else(|| v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
            .unwrap_or(v);
        return Some(v.trim().to_string());
    }
    None
}

/// Every skill installed under `<workspace>/.ferric/skills/`.
///
/// A malformed skill is reported, not fatal: one bad directory must not hide
/// the rest. Returns (skills, errors) so the caller can surface both — a silent
/// drop here would be the "reported absence without a reason" failure this
/// codebase keeps finding (ADR-088/090).
pub fn discover(workspace_root: &Path) -> (Vec<DiscoveredSkill>, Vec<SkillError>) {
    let root = skills_dir(workspace_root);
    let mut found = Vec::new();
    let mut errors = Vec::new();

    let entries = match std::fs::read_dir(&root) {
        Ok(e) => e,
        // No skills directory is a normal state, not an error.
        Err(_) => return (found, errors),
    };

    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();

    for dir in dirs {
        let Some(dir_name) = dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let file = dir.join(SKILL_FILE);
        if !file.is_file() {
            continue;
        }
        match std::fs::read_to_string(&file) {
            Ok(text) => match parse_skill(&text, dir_name, file.clone()) {
                Ok(s) => found.push(s),
                Err(e) => errors.push(e),
            },
            Err(e) => errors.push(SkillError::Io(file.display().to_string(), e.to_string())),
        }
    }

    (found, errors)
}

/// Clear skills to run.
///
/// `requested` are names the user named on this invocation; `allowed` is the
/// user's configured allowlist. A skill needs to appear in one of them.
/// Requested names that match nothing on disk are returned as `unknown`, so a
/// typo surfaces instead of silently running nothing.
///
/// Note the shape: this takes the *user's* inputs and the discovered set. There
/// is no parameter through which a model could contribute.
pub fn authorize(
    discovered: &[DiscoveredSkill],
    requested: &[String],
    allowed: &[String],
) -> (Vec<AuthorizedSkill>, Vec<String>) {
    let mut out = Vec::new();
    let mut unknown = Vec::new();

    for name in requested {
        // Dedup against what is already cleared. Repeating `--skill x` used to
        // push a second copy, so the instructions appeared twice in the prompt
        // — wasted context and a quiet shift in emphasis. The `allowed` loop
        // below always had this check; the requested loop did not have it
        // against itself.
        if out.iter().any(|a: &AuthorizedSkill| a.name() == name) {
            continue;
        }
        match discovered.iter().find(|s| &s.name == name) {
            Some(s) => out.push(AuthorizedSkill {
                skill: s.clone(),
                authority: Authority::UserRequested,
            }),
            // A repeated *unknown* name is also reported once.
            None if !unknown.contains(name) => unknown.push(name.clone()),
            None => {}
        }
    }

    for name in allowed {
        if out.iter().any(|a| a.name() == name) {
            continue; // already cleared, and UserRequested is the stronger reason
        }
        if let Some(s) = discovered.iter().find(|s| &s.name == name) {
            out.push(AuthorizedSkill {
                skill: s.clone(),
                authority: Authority::ConfigAllowed,
            });
        }
        // An allowlisted skill that is not installed is not an error: the
        // allowlist is a standing preference and may name skills for other
        // workspaces.
    }

    (out, unknown)
}

/// Render authorized skills as a prompt section.
///
/// Takes `AuthorizedSkill`, never `DiscoveredSkill` — the type system is what
/// enforces that an unauthorized skill cannot reach a prompt. Returns `None`
/// when nothing is authorized, so callers append nothing rather than an empty
/// heading.
pub fn compose(skills: &[AuthorizedSkill]) -> Option<String> {
    if skills.is_empty() {
        return None;
    }
    let mut s = String::from(
        "## Available skills\n\nThe user has installed and authorized these skills for this session. Follow a skill's instructions when its description matches the task at hand.\n",
    );
    for sk in skills {
        s.push_str(&format!(
            "\n### {} — {}\n\n{}\n",
            sk.name(),
            sk.description(),
            sk.instructions()
        ));
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_skill(root: &Path, name: &str, body: &str) {
        let dir = skills_dir(root).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(SKILL_FILE), body).unwrap();
    }

    fn valid(name: &str) -> String {
        format!("---\nname: {name}\ndescription: Does a thing\n---\n\nStep one.\nStep two.\n")
    }

    #[test]
    fn parses_the_conventional_format() {
        let s = parse_skill(&valid("tidy"), "tidy", PathBuf::from("SKILL.md")).unwrap();
        assert_eq!(s.name, "tidy");
        assert_eq!(s.description, "Does a thing");
        assert_eq!(s.instructions, "Step one.\nStep two.");
    }

    #[test]
    fn tolerates_quotes_crlf_and_a_bom() {
        let text =
            "\u{feff}---\r\nname: \"tidy\"\r\ndescription: 'Does a thing'\r\n---\r\nBody\r\n";
        let s = parse_skill(text, "tidy", PathBuf::from("SKILL.md")).unwrap();
        assert_eq!(s.name, "tidy");
        assert_eq!(s.description, "Does a thing");
    }

    #[test]
    fn rejects_malformed_frontmatter() {
        let cases = [
            ("no frontmatter at all", "just prose\n"),
            ("unterminated", "---\nname: x\n"),
            ("missing description", "---\nname: tidy\n---\nbody"),
            ("empty name", "---\nname:\ndescription: d\n---\nbody"),
        ];
        for (label, text) in cases {
            assert!(
                parse_skill(text, "tidy", PathBuf::from("SKILL.md")).is_err(),
                "{label} must be rejected"
            );
        }
    }

    /// A skill is addressed by directory name. If the declared name could
    /// differ, installing `helpful` could put a skill on disk that answers to
    /// `deploy` — so the two must agree.
    #[test]
    fn a_declared_name_must_match_its_directory() {
        let err = parse_skill(&valid("deploy"), "helpful", PathBuf::from("SKILL.md")).unwrap_err();
        assert!(matches!(err, SkillError::NameMismatch(..)), "got {err:?}");
    }

    #[test]
    fn names_are_restricted_to_a_safe_charset() {
        for bad in [
            "../escape",
            "has space",
            "UPPER",
            "semi;colon",
            "",
            &"x".repeat(65),
        ] {
            assert!(!is_valid_name(bad), "{bad:?} must be rejected");
        }
        for good in ["tidy", "run-tests", "deploy_v2", "a1"] {
            assert!(is_valid_name(good), "{good:?} must be accepted");
        }
    }

    #[test]
    fn discovery_reports_bad_skills_without_hiding_good_ones() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(tmp.path(), "good", &valid("good"));
        write_skill(tmp.path(), "bad", "no frontmatter here\n");

        let (found, errors) = discover(tmp.path());
        assert_eq!(found.len(), 1, "the good skill must still load");
        assert_eq!(found[0].name, "good");
        assert_eq!(errors.len(), 1, "the bad one must be reported, not dropped");
    }

    #[test]
    fn a_missing_skills_directory_is_not_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let (found, errors) = discover(tmp.path());
        assert!(found.is_empty() && errors.is_empty());
    }

    /// **The core security property.** Everything on disk is discovered; nothing
    /// is authorized without a human reason. A skill the user neither requested
    /// nor allowlisted must not be composable.
    #[test]
    fn discovery_alone_authorizes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(tmp.path(), "sneaky", &valid("sneaky"));

        let (found, _) = discover(tmp.path());
        assert_eq!(found.len(), 1, "it is on disk and visible");

        let (authorized, unknown) = authorize(&found, &[], &[]);
        assert!(
            authorized.is_empty(),
            "an installed-but-unauthorized skill must not be cleared"
        );
        assert!(unknown.is_empty());
        assert_eq!(
            compose(&authorized),
            None,
            "and must contribute nothing to the prompt"
        );
    }

    #[test]
    fn both_authorities_clear_a_skill_and_are_distinguishable() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(tmp.path(), "one", &valid("one"));
        write_skill(tmp.path(), "two", &valid("two"));
        let (found, _) = discover(tmp.path());

        let (authorized, _) = authorize(&found, &["one".into()], &["two".into()]);
        assert_eq!(authorized.len(), 2);
        let one = authorized.iter().find(|s| s.name() == "one").unwrap();
        let two = authorized.iter().find(|s| s.name() == "two").unwrap();
        assert_eq!(one.authority(), Authority::UserRequested);
        assert_eq!(two.authority(), Authority::ConfigAllowed);
    }

    /// A direct request is the stronger reason, and must not be duplicated into
    /// two entries when the skill is also allowlisted.
    #[test]
    fn a_requested_and_allowlisted_skill_is_cleared_once_as_requested() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(tmp.path(), "one", &valid("one"));
        let (found, _) = discover(tmp.path());

        let (authorized, _) = authorize(&found, &["one".into()], &["one".into()]);
        assert_eq!(authorized.len(), 1);
        assert_eq!(authorized[0].authority(), Authority::UserRequested);
    }

    /// A typo must surface. Silently running zero skills is the failure mode
    /// this codebase keeps finding — an absence reported as a clean no-op.
    #[test]
    fn a_requested_skill_that_is_not_installed_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(tmp.path(), "tidy", &valid("tidy"));
        let (found, _) = discover(tmp.path());

        let (authorized, unknown) = authorize(&found, &["tidyy".into()], &[]);
        assert!(authorized.is_empty());
        assert_eq!(unknown, vec!["tidyy".to_string()]);
    }

    /// An allowlist naming a skill that is not installed here is fine — the
    /// allowlist is a standing preference across workspaces, not an assertion
    /// that every entry exists.
    #[test]
    fn an_allowlisted_but_uninstalled_skill_is_quietly_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let (found, _) = discover(tmp.path());
        let (authorized, unknown) = authorize(&found, &[], &["elsewhere".into()]);
        assert!(authorized.is_empty() && unknown.is_empty());
    }

    #[test]
    fn compose_renders_instructions_for_authorized_skills_only() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(tmp.path(), "shown", &valid("shown"));
        write_skill(tmp.path(), "hidden", &valid("hidden"));
        let (found, _) = discover(tmp.path());

        let (authorized, _) = authorize(&found, &["shown".into()], &[]);
        let text = compose(&authorized).expect("one skill authorized");
        assert!(text.contains("shown"));
        assert!(text.contains("Step one."));
        assert!(
            !text.contains("hidden"),
            "an unauthorized skill must not appear in the prompt:\n{text}"
        );
    }

    /// `.ferric/` is write-denied to the model (`ferric-guard`), which is what
    /// makes "only the user installs" true. This pins the location so a future
    /// move out from under that denial has to be deliberate.
    #[test]
    fn skills_live_under_the_model_immutable_ferric_dir() {
        let dir = skills_dir(Path::new("/ws"));
        assert_eq!(dir, Path::new("/ws").join(".ferric").join("skills"));
    }

    /// `--skill marker --skill marker` must clear it once.
    ///
    /// The `allowed` loop already dedups against what is cleared; the
    /// `requested` loop did not dedup against *itself*, so repeating a flag
    /// injected the same instructions twice — wasted context, and a silent
    /// change in emphasis for anyone who repeats a flag out of habit.
    #[test]
    fn repeating_a_requested_skill_clears_it_once() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(tmp.path(), "one", &valid("one"));
        let (found, _) = discover(tmp.path());

        let (authorized, unknown) = authorize(&found, &["one".into(), "one".into()], &[]);
        assert_eq!(
            authorized.len(),
            1,
            "a repeated --skill must not duplicate the instructions"
        );
        assert!(unknown.is_empty());

        let text = compose(&authorized).unwrap();
        assert_eq!(
            text.matches("Step one.").count(),
            1,
            "the body must appear once:
{text}"
        );
    }

    /// A `--skill` value that looks like a path must simply not match. Names are
    /// compared for equality against what was discovered, so traversal has no
    /// route here — this pins that it stays an ordinary miss rather than
    /// becoming a lookup.
    #[test]
    fn a_path_shaped_skill_name_is_just_unknown() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(tmp.path(), "one", &valid("one"));
        let (found, _) = discover(tmp.path());

        let (authorized, unknown) = authorize(&found, &["../../etc/passwd".into()], &[]);
        assert!(authorized.is_empty());
        assert_eq!(unknown.len(), 1);
    }
}

//! The L0–L6 benchmark spec model (TOML), ported from the Animus ladder with
//! Ferric tool names. Specs are embedded via `include_str!` and parsed at
//! runtime; `deny_unknown_fields` rejects typos.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BenchSpec {
    /// Increment when a level's prompt or authoritative grading contract
    /// changes. Specs written before versioning deserialize as version 1.
    #[serde(default = "default_spec_version")]
    pub version: u32,
    pub level: u8,
    pub name: String,
    pub prompt: String,
    /// Files materialized into the workspace before the run (path → content).
    #[serde(default)]
    pub setup_files: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub expectations: Vec<Expectation>,
    /// Tools that MUST all be called.
    #[serde(default)]
    pub expected_tools: Vec<String>,
    /// At least one of these must be called (empty = no constraint).
    #[serde(default)]
    pub any_of_tools: Vec<String>,
    /// Tools that must NOT be called.
    #[serde(default)]
    pub forbidden_tools: Vec<String>,
    /// Trusted, fixed-argv post-run checks. `argv[0]` is the executable; the
    /// special value `{python}` resolves to `bench full --python-bin`.
    #[serde(default)]
    pub checks: Vec<CommandCheck>,
    pub max_turns: u32,
    pub timeout_s: u64,
}

pub(crate) const fn default_spec_version() -> u32 {
    1
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CommandCheck {
    pub name: String,
    pub argv: Vec<String>,
    #[serde(default = "default_expected_exit")]
    pub expected_exit: i32,
    #[serde(default)]
    pub stdout_regex: Option<String>,
    #[serde(default)]
    pub stderr_regex: Option<String>,
    #[serde(default = "default_check_timeout")]
    pub timeout_s: u64,
}

const fn default_expected_exit() -> i32 {
    0
}

const fn default_check_timeout() -> u64 {
    15
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Expectation {
    pub path: String,
    #[serde(rename = "type")]
    pub kind: ExpectKind,
    /// Optional regex the file content must match (file expectations only).
    #[serde(default)]
    pub content_regex: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExpectKind {
    File,
    Dir,
    Missing,
}

/// The embedded ladder. L0/L3/L4 are runnable with the s2 toolset; L1/L2 use
/// move_path/make_dir (added in T-205); L5/L6 need richer verification and are
/// included as specs but flagged for later (they have no post-verify runner
/// yet — they exercise file creation only).
pub const EMBEDDED_SPECS: &[&str] = &[
    include_str!("../specs/l0.toml"),
    include_str!("../specs/l1.toml"),
    include_str!("../specs/l2.toml"),
    include_str!("../specs/l3.toml"),
    include_str!("../specs/l4.toml"),
    include_str!("../specs/l5.toml"),
    include_str!("../specs/l6.toml"),
];

/// Parse all embedded specs, sorted by level.
pub fn embedded_specs() -> Result<Vec<BenchSpec>, toml::de::Error> {
    let mut specs: Vec<BenchSpec> = EMBEDDED_SPECS
        .iter()
        .map(|s| toml::from_str(s))
        .collect::<Result<_, _>>()?;
    specs.sort_by_key(|s| s.level);
    Ok(specs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_seven_specs_parse() {
        let specs = embedded_specs().expect("all specs parse");
        assert_eq!(specs.len(), 7);
        let levels: Vec<u8> = specs.iter().map(|s| s.level).collect();
        assert_eq!(levels, vec![0, 1, 2, 3, 4, 5, 6]);
        assert_eq!(specs[0].version, 1, "legacy specs default to version 1");
        assert_eq!(
            specs[3..].iter().map(|s| s.version).collect::<Vec<_>>(),
            vec![2, 2, 2, 2]
        );
        assert_eq!(
            specs[3..]
                .iter()
                .map(|s| s.checks.len())
                .collect::<Vec<_>>(),
            vec![2, 3, 3, 10]
        );
        assert!(
            specs[3..]
                .iter()
                .flat_map(|s| &s.checks)
                .all(|check| { check.argv.first().map(String::as_str) == Some("{python}") })
        );
    }

    #[test]
    fn l0_forbids_mutations_and_uses_ferric_tool_names() {
        let specs = embedded_specs().unwrap();
        let l0 = &specs[0];
        for forbidden in ["write_file", "move_path", "make_dir"] {
            assert!(
                l0.forbidden_tools.iter().any(|t| t == forbidden),
                "L0 must forbid {forbidden}"
            );
        }
        assert!(l0.expected_tools.iter().any(|t| t == "task_complete"));
        assert!(l0.any_of_tools.iter().any(|t| t == "list_dir"));
    }

    #[test]
    fn l1_l2_use_ferric_mutation_tools() {
        let specs = embedded_specs().unwrap();
        assert!(specs[1].any_of_tools.iter().any(|t| t == "move_path"));
        assert!(specs[2].any_of_tools.iter().any(|t| t == "make_dir"));
    }

    #[test]
    fn unknown_field_rejected() {
        let bad = r#"
            level = 0
            name = "x"
            prompt = "p"
            max_turns = 5
            timeout_s = 60
            bogus_field = true
        "#;
        assert!(toml::from_str::<BenchSpec>(bad).is_err());
    }
}

//! The L0–L6 benchmark spec model (TOML), ported from the Animus ladder with
//! Ferric tool names. Specs are embedded via `include_str!` and parsed at
//! runtime; `deny_unknown_fields` rejects typos.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BenchSpec {
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
    pub max_turns: u32,
    pub timeout_s: u64,
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

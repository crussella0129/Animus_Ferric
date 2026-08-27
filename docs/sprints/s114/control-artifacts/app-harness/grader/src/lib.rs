//! Static, dependency-free preflight grading for the MH-RS01 candidate.
//!
//! This crate deliberately does not execute candidate code. The operator-owned
//! sandbox driver must require a successful [`grade_candidate`] result before
//! it invokes Cargo inside Bubblewrap.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path};

pub const IMMUTABLE_PATHS: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "README.md",
    "src/lib.rs",
    "tests/contract.rs",
];

pub const MUTABLE_PATHS: &[&str] = &[
    "PLAN.md",
    "src/model.rs",
    "src/parser.rs",
    "src/scheduler.rs",
    "src/main.rs",
    "tests/agent_tests.rs",
];

const DIMENSIONS: &[&str] = &[
    "seed_immutability",
    "dependency_policy",
    "path_policy",
    "plan",
    "model_tests",
    "source_safety",
];

const MAX_RETAINED_DETAILS: usize = 2;
const MAX_DETAIL_BYTES: usize = 96;
const TRUNCATED_DETAIL_SUFFIX: &str = "...[truncated]";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DimensionResult {
    pub dimension: &'static str,
    pub passed: bool,
    pub details: Vec<String>,
    additional_violations: usize,
}

impl DimensionResult {
    fn new(dimension: &'static str) -> Self {
        Self {
            dimension,
            passed: true,
            details: Vec::new(),
            additional_violations: 0,
        }
    }

    fn fail(&mut self, detail: impl Into<String>) {
        self.passed = false;
        let detail = bounded_detail(detail.into());
        if self.details.contains(&detail) {
            self.additional_violations = self.additional_violations.saturating_add(1);
            return;
        }
        if self.details.len() < MAX_RETAINED_DETAILS {
            self.details.push(detail);
        } else {
            self.additional_violations = self.additional_violations.saturating_add(1);
        }
    }

    fn finish(&mut self) {
        self.details.sort();
        if self.additional_violations != 0 {
            self.details.push(format!(
                "additional_violations: {}",
                self.additional_violations
            ));
            self.details.sort();
        }
    }
}

fn bounded_detail(mut detail: String) -> String {
    if detail.len() <= MAX_DETAIL_BYTES {
        return detail;
    }
    let mut prefix = MAX_DETAIL_BYTES - TRUNCATED_DETAIL_SUFFIX.len();
    while !detail.is_char_boundary(prefix) {
        prefix -= 1;
    }
    detail.truncate(prefix);
    detail.push_str(TRUNCATED_DETAIL_SUFFIX);
    detail
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GradeReport {
    pub dimensions: Vec<DimensionResult>,
}

impl GradeReport {
    pub fn execution_allowed(&self) -> bool {
        self.dimensions.iter().all(|result| result.passed)
    }

    /// Return deterministic JSON Lines. Dimensions have a fixed order and all
    /// detail strings are sorted and deduplicated before serialization.
    pub fn to_jsonl(&self) -> String {
        let mut output = String::new();
        for result in &self.dimensions {
            let _ = write!(
                output,
                "{{\"schema\":\"s114-grade-v1\",\"dimension\":\"{}\",\"status\":\"{}\",\"details\":[",
                json_escape(result.dimension),
                if result.passed { "pass" } else { "fail" }
            );
            for (index, detail) in result.details.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                let _ = write!(output, "\"{}\"", json_escape(detail));
            }
            output.push_str("]}\n");
        }
        output
    }
}

/// Grade the candidate's complete static surface against the frozen seed.
///
/// An I/O error means the grader itself could not establish the boundary. A
/// policy violation is represented as a successful report containing one or
/// more failed dimensions. Callers must never execute candidate code unless
/// [`GradeReport::execution_allowed`] is true.
pub fn grade_candidate(candidate: &Path, seed: &Path) -> io::Result<GradeReport> {
    let mut results: Vec<_> = DIMENSIONS
        .iter()
        .copied()
        .map(DimensionResult::new)
        .collect();

    let candidate_meta = fs::symlink_metadata(candidate)?;
    if candidate_meta.file_type().is_symlink() || !candidate_meta.is_dir() {
        let reason = if candidate_meta.file_type().is_symlink() {
            "candidate root is a symlink"
        } else {
            "candidate root is not a directory"
        };
        result_mut(&mut results, "path_policy").fail(reason);
        for result in &mut results {
            if result.dimension != "path_policy" {
                result.fail("not evaluated because candidate root is invalid");
            }
            result.finish();
        }
        return Ok(GradeReport {
            dimensions: results,
        });
    }
    let seed_meta = fs::symlink_metadata(seed)?;
    if !seed_meta.is_dir() || seed_meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "seed path must be a real directory",
        ));
    }

    let mut inventory_safe_to_read = true;
    let entries = collect_candidate_entries(
        candidate,
        result_mut(&mut results, "path_policy"),
        &mut inventory_safe_to_read,
    )?;
    check_allowlist(candidate, &entries, &mut results, inventory_safe_to_read);
    check_symlinks(&entries, &mut results);
    if !inventory_safe_to_read {
        for result in &mut results {
            if result.dimension != "path_policy" {
                result.fail("not evaluated because candidate inventory is unsafe");
            }
            result.finish();
        }
        return Ok(GradeReport {
            dimensions: results,
        });
    }
    check_immutable(candidate, seed, &mut results)?;
    check_dependencies(candidate, &mut results)?;
    check_plan(candidate, &mut results)?;
    check_test_count(candidate, &mut results)?;
    check_source_safety(candidate, &mut results)?;

    for result in &mut results {
        result.finish();
    }
    Ok(GradeReport {
        dimensions: results,
    })
}

#[derive(Debug)]
struct CandidateEntry {
    relative: String,
    kind: EntryKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EntryKind {
    File,
    Directory,
    Symlink,
    Other,
}

fn collect_candidate_entries(
    root: &Path,
    path_result: &mut DimensionResult,
    safe_to_read: &mut bool,
) -> io::Result<Vec<CandidateEntry>> {
    const MAX_ENTRIES: usize = 4_096;
    const MAX_RELATIVE_PATH_BYTES: usize = 4_096;

    let mut entries = Vec::new();
    let mut pending = vec![(root.to_path_buf(), String::new())];
    'walk: while let Some((directory, relative_directory)) = pending.pop() {
        let remaining_capacity = MAX_ENTRIES.saturating_sub(entries.len());
        let mut children = Vec::with_capacity(remaining_capacity.min(256));
        for child in fs::read_dir(&directory)?.take(remaining_capacity + 1) {
            children.push(child?);
        }
        if children.len() > remaining_capacity {
            *safe_to_read = false;
            path_result.fail(format!(
                "filesystem entry limit exceeded: more than {MAX_ENTRIES}"
            ));
            break 'walk;
        }
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            let name = child.file_name().to_string_lossy().into_owned();
            let relative = if relative_directory.is_empty() {
                name
            } else {
                format!("{relative_directory}/{name}")
            };
            if relative.len() > MAX_RELATIVE_PATH_BYTES {
                *safe_to_read = false;
                path_result.fail(format!(
                    "relative path length limit exceeded: more than {MAX_RELATIVE_PATH_BYTES} bytes"
                ));
                continue;
            }
            let path = child.path();
            let metadata = fs::symlink_metadata(&path)?;
            let kind = if metadata.file_type().is_symlink() {
                EntryKind::Symlink
            } else if metadata.is_file() {
                EntryKind::File
            } else if metadata.is_dir() {
                EntryKind::Directory
            } else {
                EntryKind::Other
            };
            if matches!(kind, EntryKind::Symlink | EntryKind::Other) {
                *safe_to_read = false;
            }
            if kind == EntryKind::File && has_multiple_hard_links(&metadata) {
                *safe_to_read = false;
                path_result.fail(format!("hardlink: {relative}"));
            }
            entries.push(CandidateEntry {
                relative: relative.clone(),
                kind,
            });

            if kind == EntryKind::Directory {
                pending.push((path, relative));
            }
        }
    }
    entries.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(entries)
}

#[cfg(unix)]
fn has_multiple_hard_links(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    metadata.nlink() > 1
}

#[cfg(not(unix))]
fn has_multiple_hard_links(_metadata: &fs::Metadata) -> bool {
    false
}

fn check_allowlist(
    root: &Path,
    entries: &[CandidateEntry],
    results: &mut [DimensionResult],
    inspect_required_paths: bool,
) {
    let result = result_mut(results, "path_policy");
    let allowed_files: BTreeSet<_> = IMMUTABLE_PATHS
        .iter()
        .chain(MUTABLE_PATHS.iter())
        .copied()
        .collect();
    let allowed_directories = BTreeSet::from(["src", "tests"]);

    for entry in entries {
        match entry.kind {
            EntryKind::File if !allowed_files.contains(entry.relative.as_str()) => {
                result.fail(format!("extra path: {}", entry.relative));
            }
            EntryKind::Directory if !allowed_directories.contains(entry.relative.as_str()) => {
                result.fail(format!("extra directory: {}", entry.relative));
            }
            EntryKind::Other => result.fail(format!("special path: {}", entry.relative)),
            _ => {}
        }
    }

    if !inspect_required_paths {
        return;
    }
    for required in allowed_files {
        match fs::symlink_metadata(root.join(required)) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
            Ok(_) => result.fail(format!("required path is not a regular file: {required}")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                result.fail(format!("missing required path: {required}"));
            }
            Err(error) => result.fail(format!("cannot inspect {required}: {error}")),
        }
    }
}

fn check_symlinks(entries: &[CandidateEntry], results: &mut [DimensionResult]) {
    let result = result_mut(results, "path_policy");
    for entry in entries {
        if entry.kind == EntryKind::Symlink {
            result.fail(format!("symlink: {}", entry.relative));
        }
    }
}

fn check_immutable(
    candidate: &Path,
    seed: &Path,
    results: &mut [DimensionResult],
) -> io::Result<()> {
    let result = result_mut(results, "seed_immutability");
    for relative in IMMUTABLE_PATHS {
        let candidate_path = candidate.join(relative);
        let seed_path = seed.join(relative);
        let candidate_meta = match fs::symlink_metadata(&candidate_path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata,
            Ok(_) => {
                result.fail(format!(
                    "candidate immutable is not a regular file: {relative}"
                ));
                continue;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                result.fail(format!("candidate immutable missing: {relative}"));
                continue;
            }
            Err(error) => return Err(error),
        };
        let seed_meta = match fs::symlink_metadata(&seed_path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata,
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("seed immutable is not a regular file: {relative}"),
                ));
            }
            Err(error) => return Err(error),
        };
        if candidate_meta.len() != seed_meta.len()
            || sha256_file(&candidate_path)? != sha256_file(&seed_path)?
        {
            result.fail(format!("immutable mismatch: {relative}"));
        }
    }
    Ok(())
}

fn check_dependencies(candidate: &Path, results: &mut [DimensionResult]) -> io::Result<()> {
    let result = result_mut(results, "dependency_policy");
    let manifest_path = candidate.join("Cargo.toml");
    let metadata = match regular_file_metadata(&manifest_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            result.fail("Cargo.toml is missing");
            return Ok(());
        }
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            result.fail("Cargo.toml is not a regular file");
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    if metadata.len() > 128 * 1024 {
        result.fail("Cargo.toml exceeds 128 KiB");
        return Ok(());
    }
    let manifest = match fs::read_to_string(&manifest_path) {
        Ok(manifest) => manifest,
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            result.fail("Cargo.toml is not UTF-8");
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let mut section = String::new();
    for (line_index, raw_line) in manifest.lines().enumerate() {
        let line = strip_toml_comment(raw_line).trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line
                .trim_start_matches('[')
                .trim_end_matches(']')
                .trim()
                .to_ascii_lowercase();
            if section == "package.metadata" {
                continue;
            }
            if section.contains("dependencies") {
                // Empty dependency tables are permitted; entries are rejected
                // below. Inline target dependency tables are rejected here.
                if section.starts_with("target.") {
                    result.fail(format!(
                        "target-specific dependency section at line {}",
                        line_index + 1
                    ));
                }
            }
            continue;
        }
        if line.is_empty() {
            continue;
        }
        if section.contains("dependencies") && line.contains('=') {
            result.fail(format!("dependency entry at line {}", line_index + 1));
        }
        if section == "package"
            && line
                .split_once('=')
                .is_some_and(|(key, _)| key.trim().eq_ignore_ascii_case("build"))
        {
            result.fail(format!("package build script at line {}", line_index + 1));
        }
    }
    Ok(())
}

fn check_plan(candidate: &Path, results: &mut [DimensionResult]) -> io::Result<()> {
    let result = result_mut(results, "plan");
    let path = candidate.join("PLAN.md");
    let metadata = match regular_file_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            result.fail("PLAN.md is missing");
            return Ok(());
        }
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            result.fail("PLAN.md is not a regular file");
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    if metadata.len() > 256 * 1024 {
        result.fail("PLAN.md exceeds 256 KiB");
        return Ok(());
    }
    let plan = match fs::read_to_string(path) {
        Ok(plan) => plan,
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            result.fail("PLAN.md is not UTF-8");
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    for heading in ["## Contract", "## File plan", "## Verification"] {
        if !plan.lines().any(|line| line.trim() == heading) {
            result.fail(format!("missing heading: {heading}"));
        }
    }

    let mut checked_labels = Vec::new();
    let mut unchecked = 0usize;
    for line in plan.lines() {
        let trimmed = line.trim_start();
        let list_body = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "));
        let Some(list_body) = list_body else {
            continue;
        };
        if let Some(label) = list_body
            .strip_prefix("[x]")
            .or_else(|| list_body.strip_prefix("[X]"))
        {
            checked_labels.push(label.trim().to_ascii_lowercase());
        } else if list_body.starts_with("[ ]") {
            unchecked += 1;
        }
    }
    if unchecked != 0 {
        result.fail(format!("incomplete checklist items: {unchecked}"));
    }
    for required in [
        "model.rs",
        "parser.rs",
        "scheduler.rs",
        "main.rs",
        "agent_tests.rs",
    ] {
        if !checked_labels.iter().any(|label| label.contains(required)) {
            result.fail(format!("completed checklist does not cover {required}"));
        }
    }
    if !checked_labels.iter().any(|label| {
        label.contains("authorized check")
            || label.contains("run check")
            || label.contains("cargo test")
            || label.contains("verification run")
    }) {
        result.fail("completed checklist does not cover the authorized test run");
    }
    Ok(())
}

fn check_test_count(candidate: &Path, results: &mut [DimensionResult]) -> io::Result<()> {
    let result = result_mut(results, "model_tests");
    let path = candidate.join("tests/agent_tests.rs");
    let metadata = match regular_file_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            result.fail("tests/agent_tests.rs is missing");
            return Ok(());
        }
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            result.fail("tests/agent_tests.rs is not a regular file");
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    if metadata.len() > 1024 * 1024 {
        result.fail("tests/agent_tests.rs exceeds 1 MiB");
        return Ok(());
    }
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            result.fail("tests/agent_tests.rs is not UTF-8");
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let sanitized = sanitize_rust(&source);
    for relative in IMMUTABLE_PATHS
        .iter()
        .chain(MUTABLE_PATHS.iter())
        .copied()
        .filter(|relative| relative.ends_with(".rs"))
    {
        let candidate_source = candidate.join(relative);
        let Ok(candidate_text) = fs::read_to_string(candidate_source) else {
            continue;
        };
        if contains_oracle_alias(&sanitize_rust(&candidate_text)) {
            result.fail(format!("oracle macro alias is forbidden: {relative}"));
        }
    }
    let extracted = extract_test_functions(&sanitized);
    let test_names: BTreeSet<_> = extracted
        .functions
        .iter()
        .map(|test| test.name.clone())
        .collect();
    let test_count = test_names.len();
    if test_count < 6 {
        result.fail(format!(
            "focused test count is {test_count}; expected at least 6"
        ));
    }
    let test_identifiers: Vec<_> = rust_identifiers(&sanitized).collect();
    let attribute_scan = scan_rust_attributes(&sanitized);
    if attribute_scan.malformed != 0 {
        result.fail(format!(
            "model-authored tests contain malformed Rust attributes: {}",
            attribute_scan.malformed
        ));
    }
    if attribute_scan
        .bodies
        .iter()
        .any(|attribute| test_attribute_is_disabled(attribute))
    {
        result.fail("model-authored tests contain an explicitly disabled or ignored test");
    }
    if extracted.malformed_attributes != 0 {
        result.fail(format!(
            "test attributes without extractable function bodies: {}",
            extracted.malformed_attributes
        ));
    }
    for test in &extracted.functions {
        if !test_body_has_oracle(test) {
            result.fail(format!(
                "model-authored test has no oracle signal: {}",
                test.name
            ));
        }
        if (topic_parsing(&test.name) || topic_invalid_dependencies(&test.name))
            && !contains_coupled_api_oracle(test.body, "parse_manifest")
        {
            result.fail(format!(
                "topical test lacks a coupled ::release_plan::parse_manifest oracle: {}",
                test.name
            ));
        }
        if (topic_completed_prerequisites(&test.name)
            || topic_priority_ordering(&test.name)
            || topic_lexical_tie_breaking(&test.name)
            || topic_cycles(&test.name)
            || topic_input_preservation(&test.name))
            && !contains_coupled_api_oracle(test.body, "build_plan")
        {
            result.fail(format!(
                "topical test lacks a coupled ::release_plan::build_plan oracle: {}",
                test.name
            ));
        }
    }
    for required_api in ["parse_manifest", "build_plan"] {
        if !test_identifiers.contains(&required_api) {
            result.fail(format!(
                "model-authored test suite does not reference required API: {required_api}"
            ));
        }
    }

    for (topic, predicate) in [
        ("parsing", topic_parsing as fn(&str) -> bool),
        ("invalid dependencies", topic_invalid_dependencies),
        ("completed prerequisites", topic_completed_prerequisites),
        ("priority ordering", topic_priority_ordering),
        ("lexical tie-breaking", topic_lexical_tie_breaking),
        ("cycles", topic_cycles),
        ("input preservation", topic_input_preservation),
    ] {
        let covered = test_names.iter().any(|name| predicate(name));
        if !covered {
            result.fail(format!(
                "model-authored tests do not name disclosed topic: {topic}"
            ));
        }
    }
    Ok(())
}

fn topic_parsing(name: &str) -> bool {
    name.contains("pars")
        || (name.contains("manifest") && (name.contains("valid") || name.contains("accept")))
}

fn topic_invalid_dependencies(name: &str) -> bool {
    (name.contains("depend")
        && [
            "invalid",
            "reject",
            "unknown",
            "duplicate",
            "self",
            "empty",
            "error",
        ]
        .iter()
        .any(|marker| name.contains(marker)))
        || (["unknown", "duplicate", "self", "empty"]
            .iter()
            .any(|marker| name.contains(marker))
            && (name.contains("reject") || name.contains("invalid")))
}

fn topic_completed_prerequisites(name: &str) -> bool {
    name.contains("completed")
        || (name.contains("done")
            && ["prereq", "depend", "job", "omit", "satisf", "unlock"]
                .iter()
                .any(|marker| name.contains(marker)))
}

fn topic_priority_ordering(name: &str) -> bool {
    name.contains("priority") || (name.contains("highest") && name.contains("ready"))
}

fn topic_lexical_tie_breaking(name: &str) -> bool {
    name.contains("lexical") || name.contains("tie") || name.contains("alphabet")
}

fn topic_cycles(name: &str) -> bool {
    name.contains("cycle") || name.contains("deadlock")
}

fn topic_input_preservation(name: &str) -> bool {
    (name.contains("preserv")
        && (name.contains("input") || name.contains("job") || name.contains("manifest")))
        || (name.contains("not") && name.contains("mutat"))
}

#[derive(Debug)]
struct ExtractedTests<'a> {
    functions: Vec<ExtractedTest<'a>>,
    malformed_attributes: usize,
}

#[derive(Debug)]
struct ExtractedTest<'a> {
    name: String,
    body: &'a str,
    returns_result: bool,
}

fn extract_test_functions(source: &str) -> ExtractedTests<'_> {
    const ATTRIBUTE: &str = "#[test]";

    let mut functions = Vec::new();
    let mut malformed_attributes = 0usize;
    let mut cursor = 0usize;
    while let Some(relative_attribute) = source[cursor..].find(ATTRIBUTE) {
        let attribute = cursor + relative_attribute;
        let search_start = attribute + ATTRIBUTE.len();
        let next_attribute = source[search_start..]
            .find(ATTRIBUTE)
            .map(|relative| search_start + relative);
        let Some((function_keyword, function_keyword_end)) =
            find_identifier(source, search_start, "fn")
        else {
            malformed_attributes += 1;
            break;
        };
        if next_attribute.is_some_and(|next| next < function_keyword) {
            malformed_attributes += 1;
            cursor = next_attribute.expect("checked as present");
            continue;
        }
        let Some((_, name_end, name)) = next_identifier(source, function_keyword_end) else {
            malformed_attributes += 1;
            cursor = function_keyword_end;
            continue;
        };
        let Some(relative_opening_brace) = source[name_end..].find('{') else {
            malformed_attributes += 1;
            cursor = name_end;
            continue;
        };
        let opening_brace = name_end + relative_opening_brace;
        if source[name_end..opening_brace].contains(';') {
            malformed_attributes += 1;
            cursor = opening_brace + 1;
            continue;
        }
        let Some(closing_brace) = matching_brace(source, opening_brace) else {
            malformed_attributes += 1;
            break;
        };
        let signature = &source[function_keyword..opening_brace];
        let signature_identifiers: BTreeSet<_> = rust_identifiers(signature).collect();
        functions.push(ExtractedTest {
            name: name.to_ascii_lowercase(),
            body: &source[opening_brace + 1..closing_brace],
            returns_result: signature.contains("->")
                && signature_identifiers
                    .iter()
                    .any(|identifier| identifier.ends_with("Result")),
        });
        cursor = closing_brace + 1;
    }
    ExtractedTests {
        functions,
        malformed_attributes,
    }
}

fn find_identifier(source: &str, start: usize, expected: &str) -> Option<(usize, usize)> {
    let mut cursor = start;
    while let Some((identifier_start, identifier_end, identifier)) = next_identifier(source, cursor)
    {
        if identifier == expected {
            return Some((identifier_start, identifier_end));
        }
        cursor = identifier_end;
    }
    None
}

fn next_identifier(source: &str, start: usize) -> Option<(usize, usize, &str)> {
    let bytes = source.as_bytes();
    let mut cursor = start;
    while cursor < bytes.len() && !is_identifier_byte(bytes[cursor]) {
        cursor += 1;
    }
    if cursor == bytes.len() {
        return None;
    }
    let identifier_start = cursor;
    while cursor < bytes.len() && is_identifier_byte(bytes[cursor]) {
        cursor += 1;
    }
    Some((identifier_start, cursor, &source[identifier_start..cursor]))
}

fn is_identifier_byte(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

fn matching_brace(source: &str, opening: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(opening) != Some(&b'{') {
        return None;
    }
    let mut depth = 0usize;
    for (offset, byte) in bytes[opening..].iter().copied().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(opening + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn test_body_has_oracle(test: &ExtractedTest<'_>) -> bool {
    let compact: String = test
        .body
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    contains_oracle_macro(&compact)
        || [".unwrap(", ".unwrap_err(", ".expect(", ".expect_err("]
            .iter()
            .any(|pattern| compact.contains(pattern))
        || (test.returns_result && compact.contains('?'))
}

fn contains_coupled_api_oracle(source: &str, expected: &str) -> bool {
    absolute_api_calls(source, expected)
        .into_iter()
        .any(|call| {
            call_has_result_oracle(source, call.close)
                || oracle_first_arguments(source)
                    .into_iter()
                    .any(|(start, end)| call.start >= start && call.start < end)
        })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CallRange {
    start: usize,
    close: usize,
}

fn absolute_api_calls(source: &str, expected: &str) -> Vec<CallRange> {
    let pattern = format!("::release_plan::{expected}");
    let bytes = source.as_bytes();
    let mut calls = Vec::new();
    for (start, _) in source.match_indices(&pattern) {
        if start != 0 && (is_identifier_byte(bytes[start - 1]) || bytes[start - 1] == b':') {
            continue;
        }
        let mut opening = start + pattern.len();
        while bytes.get(opening).is_some_and(u8::is_ascii_whitespace) {
            opening += 1;
        }
        if bytes.get(opening) != Some(&b'(') {
            continue;
        }
        if let Some(close) = matching_balanced_delimiter(source, opening) {
            calls.push(CallRange { start, close });
        }
    }
    calls
}

fn call_has_result_oracle(source: &str, call_close: usize) -> bool {
    let bytes = source.as_bytes();
    let mut cursor = call_close + 1;
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'.') {
        return false;
    }
    cursor += 1;
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    let Some((_, identifier_end, method)) = next_identifier(source, cursor) else {
        return false;
    };
    if !matches!(method, "unwrap" | "unwrap_err" | "expect" | "expect_err") {
        return false;
    }
    cursor = identifier_end;
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    bytes.get(cursor) == Some(&b'(')
}

fn oracle_first_arguments(source: &str) -> Vec<(usize, usize)> {
    const ORACLES: &[&str] = &[
        "assert",
        "assert_eq",
        "assert_ne",
        "debug_assert",
        "debug_assert_eq",
        "debug_assert_ne",
    ];
    let bytes = source.as_bytes();
    let mut arguments = Vec::new();
    for (bang, _) in source.match_indices('!') {
        let mut name_start = bang;
        while name_start != 0 && is_identifier_byte(bytes[name_start - 1]) {
            name_start -= 1;
        }
        if !ORACLES.contains(&&source[name_start..bang]) {
            continue;
        }
        let mut opening = bang + 1;
        while bytes.get(opening).is_some_and(u8::is_ascii_whitespace) {
            opening += 1;
        }
        if !matches!(bytes.get(opening), Some(b'(' | b'[' | b'{')) {
            continue;
        }
        if let Some((first_end, _close)) = first_argument_end(source, opening) {
            arguments.push((opening + 1, first_end));
        }
    }
    arguments
}

fn first_argument_end(source: &str, opening: usize) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut stack = vec![matching_close(*bytes.get(opening)?)?];
    let mut cursor = opening + 1;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'(' | b'[' | b'{' => stack.push(matching_close(bytes[cursor])?),
            byte if Some(&byte) == stack.last() => {
                stack.pop();
                if stack.is_empty() {
                    return Some((cursor, cursor));
                }
            }
            b',' if stack.len() == 1 => {
                let close = matching_balanced_delimiter(source, opening)?;
                return Some((cursor, close));
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

fn matching_balanced_delimiter(source: &str, opening: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut stack = vec![matching_close(*bytes.get(opening)?)?];
    for (offset, byte) in bytes[opening + 1..].iter().copied().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => stack.push(matching_close(byte)?),
            candidate if Some(&candidate) == stack.last() => {
                stack.pop();
                if stack.is_empty() {
                    return Some(opening + 1 + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn matching_close(opening: u8) -> Option<u8> {
    match opening {
        b'(' => Some(b')'),
        b'[' => Some(b']'),
        b'{' => Some(b'}'),
        _ => None,
    }
}

fn contains_oracle_alias(source: &str) -> bool {
    const ORACLES: &[&str] = &[
        "assert",
        "assert_eq",
        "assert_ne",
        "debug_assert",
        "debug_assert_eq",
        "debug_assert_ne",
        "panic",
    ];
    let identifiers: Vec<_> = rust_identifiers(source).collect();
    identifiers.iter().enumerate().any(|(index, identifier)| {
        if *identifier != "as" {
            return false;
        }
        identifiers
            .get(index + 1)
            .is_some_and(|candidate| ORACLES.contains(candidate))
            || (identifiers.get(index + 1) == Some(&"r")
                && identifiers
                    .get(index + 2)
                    .is_some_and(|candidate| ORACLES.contains(candidate)))
    })
}

fn contains_oracle_macro(source: &str) -> bool {
    for (bang_index, _) in source.match_indices('!') {
        let bytes = source.as_bytes();
        let mut identifier_start = bang_index;
        while identifier_start != 0 && is_identifier_byte(bytes[identifier_start - 1]) {
            identifier_start -= 1;
        }
        let name = &source[identifier_start..bang_index];
        if matches!(
            name,
            "assert"
                | "assert_eq"
                | "assert_ne"
                | "debug_assert"
                | "debug_assert_eq"
                | "debug_assert_ne"
                | "panic"
        ) {
            return true;
        }
    }
    false
}

fn check_source_safety(candidate: &Path, results: &mut [DimensionResult]) -> io::Result<()> {
    let result = result_mut(results, "source_safety");
    for relative in IMMUTABLE_PATHS
        .iter()
        .chain(MUTABLE_PATHS.iter())
        .copied()
        .filter(|relative| relative.ends_with(".rs"))
    {
        let path = candidate.join(relative);
        let metadata = match regular_file_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                result.fail(format!("Rust source is not a regular file: {relative}"));
                continue;
            }
            Err(error) => return Err(error),
        };
        if metadata.len() > 2 * 1024 * 1024 {
            result.fail(format!("Rust source exceeds 2 MiB: {relative}"));
            continue;
        }
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                result.fail(format!("Rust source is not UTF-8: {relative}"));
                continue;
            }
            Err(error) => return Err(error),
        };
        let sanitized = sanitize_rust(&source);
        let compact: String = sanitized
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        let identifiers: BTreeSet<_> = rust_identifiers(&sanitized).collect();
        if identifiers.contains("unsafe") {
            result.fail(format!("unsafe keyword: {relative}"));
        }
        if contains_attribute_named(&sanitized, "path") {
            result.fail(format!("path source-inclusion attribute: {relative}"));
        }
        let process_termination = (identifiers.contains("std")
            && identifiers.contains("process")
            && identifiers.contains("exit"))
            || compact.contains("std::process::exit");
        if relative != "src/main.rs" {
            if process_termination {
                result.fail(format!("process termination API outside CLI: {relative}"));
            }
            for (identifier, label) in [
                ("fs", "filesystem module identifier outside CLI"),
                ("env", "environment module identifier outside CLI"),
                ("thread", "thread module identifier outside CLI"),
                ("backtrace", "backtrace module identifier outside CLI"),
                ("Path", "filesystem Path identifier outside CLI"),
                ("path", "filesystem path identifier outside CLI"),
                ("read_link", "filesystem inspection method outside CLI"),
                ("read_dir", "filesystem inspection method outside CLI"),
                ("metadata", "filesystem inspection method outside CLI"),
                (
                    "symlink_metadata",
                    "filesystem inspection method outside CLI",
                ),
                ("canonicalize", "filesystem inspection method outside CLI"),
                ("exists", "filesystem inspection method outside CLI"),
                ("try_exists", "filesystem inspection method outside CLI"),
                ("is_file", "filesystem inspection method outside CLI"),
                ("is_dir", "filesystem inspection method outside CLI"),
                ("track_caller", "caller-location attribute outside CLI"),
            ] {
                if identifiers.contains(identifier) {
                    result.fail(format!("{label}: {relative}"));
                }
            }
            if identifiers.contains("Location") && identifiers.contains("caller") {
                result.fail(format!("caller-location API outside CLI: {relative}"));
            }
        }
        for (identifier, label) in [
            ("Command", "process command API"),
            ("TcpStream", "TCP API"),
            ("TcpListener", "TCP API"),
            ("UdpSocket", "UDP API"),
            ("ToSocketAddrs", "socket address API"),
            ("UnixStream", "Unix socket API"),
            ("UnixDatagram", "Unix socket API"),
            ("UnixListener", "Unix socket API"),
            ("extern", "foreign-function interface"),
            ("include", "source inclusion macro identifier"),
            ("include_bytes", "byte inclusion macro identifier"),
            ("include_str", "text inclusion macro identifier"),
            ("asm", "inline assembly macro identifier"),
            ("global_asm", "global assembly macro identifier"),
            ("naked_asm", "naked assembly macro identifier"),
            ("macro_rules", "local macro definition"),
        ] {
            if identifiers.contains(identifier) {
                result.fail(format!("{label}: {relative}"));
            }
        }
        for (needle, label) in [
            ("std::process::Command", "process command API"),
            ("process::Command", "process command API"),
            ("Command::new", "process command construction"),
            ("std::net", "network API"),
            ("extern\"C\"", "foreign-function interface"),
            ("include!", "source inclusion macro"),
            ("include_bytes!", "byte inclusion macro"),
            ("include_str!", "text inclusion macro"),
        ] {
            if compact.contains(needle) {
                result.fail(format!("{label}: {relative}"));
            }
        }
    }

    for forbidden in ["build.rs", ".cargo"] {
        if fs::symlink_metadata(candidate.join(forbidden)).is_ok() {
            result.fail(format!("forbidden build surface: {forbidden}"));
        }
    }
    Ok(())
}

#[derive(Debug)]
struct RustAttributeScan<'a> {
    bodies: Vec<&'a str>,
    malformed: usize,
}

/// Extract outer and inner attributes with balanced square brackets. The
/// input is sanitized first, so bracket-like comments and literals cannot
/// manufacture or truncate an attribute.
fn scan_rust_attributes(source: &str) -> RustAttributeScan<'_> {
    let bytes = source.as_bytes();
    let mut bodies = Vec::new();
    let mut malformed = 0usize;
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if bytes[cursor] != b'#' {
            cursor += 1;
            continue;
        }

        let hash = cursor;
        cursor += 1;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if bytes.get(cursor) == Some(&b'!') {
            cursor += 1;
            while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                cursor += 1;
            }
        }
        if bytes.get(cursor) != Some(&b'[') {
            cursor = hash + 1;
            continue;
        }

        let opening = cursor;
        cursor += 1;
        let mut depth = 1usize;
        while cursor < bytes.len() && depth != 0 {
            match bytes[cursor] {
                b'[' => depth += 1,
                b']' => depth -= 1,
                _ => {}
            }
            cursor += 1;
        }
        if depth != 0 {
            malformed += 1;
            break;
        }
        bodies.push(&source[opening + 1..cursor - 1]);
    }
    RustAttributeScan { bodies, malformed }
}

fn attribute_leading_name(attribute: &str) -> Option<&str> {
    next_identifier(attribute, 0).map(|(_, _, name)| name)
}

fn compact_rust_tokens(source: &str) -> String {
    source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn meta_arguments(meta: &str) -> Option<&str> {
    let (_, name_end, _) = next_identifier(meta, 0)?;
    let bytes = meta.as_bytes();
    let mut opening = name_end;
    while bytes.get(opening).is_some_and(u8::is_ascii_whitespace) {
        opening += 1;
    }
    if bytes.get(opening) != Some(&b'(') {
        return None;
    }
    let closing = matching_delimiter(meta, opening, b'(', b')')?;
    meta[closing + 1..]
        .chars()
        .all(char::is_whitespace)
        .then_some(&meta[opening + 1..closing])
}

fn matching_delimiter(source: &str, opening: usize, open: u8, close: u8) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(opening) != Some(&open) {
        return None;
    }
    let mut depth = 0usize;
    for (offset, byte) in bytes[opening..].iter().copied().enumerate() {
        if byte == open {
            depth += 1;
        } else if byte == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(opening + offset);
            }
        }
    }
    None
}

fn split_top_level_arguments(arguments: &str) -> Option<Vec<&str>> {
    let bytes = arguments.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut round = 0usize;
    let mut square = 0usize;
    let mut brace = 0usize;
    for (index, byte) in bytes.iter().copied().enumerate() {
        match byte {
            b'(' => round += 1,
            b')' => round = round.checked_sub(1)?,
            b'[' => square += 1,
            b']' => square = square.checked_sub(1)?,
            b'{' => brace += 1,
            b'}' => brace = brace.checked_sub(1)?,
            b',' if round == 0 && square == 0 && brace == 0 => {
                parts.push(&arguments[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    if round != 0 || square != 0 || brace != 0 {
        return None;
    }
    parts.push(&arguments[start..]);
    Some(parts)
}

fn cfg_attr_outputs(attribute: &str) -> Option<Vec<&str>> {
    let arguments = meta_arguments(attribute)?;
    let parts = split_top_level_arguments(arguments)?;
    (parts.len() >= 2).then(|| parts.into_iter().skip(1).collect())
}

fn test_attribute_is_disabled(attribute: &str) -> bool {
    match attribute_leading_name(attribute) {
        Some("ignore") => true,
        Some("cfg") => compact_rust_tokens(attribute) != "cfg(test)",
        Some("cfg_attr") => cfg_attr_outputs(attribute)
            .is_none_or(|outputs| outputs.into_iter().any(test_attribute_is_disabled)),
        _ => false,
    }
}

fn attribute_effectively_named(attribute: &str, name: &str) -> bool {
    if attribute_leading_name(attribute) == Some(name) {
        return true;
    }
    attribute_leading_name(attribute) == Some("cfg_attr")
        && cfg_attr_outputs(attribute).is_some_and(|outputs| {
            outputs
                .into_iter()
                .any(|output| attribute_effectively_named(output, name))
        })
}

/// Detect a direct or `cfg_attr`-produced Rust attribute with this leading
/// meta-path. Whitespace, comments, and literal contents are ignored.
fn contains_attribute_named(source: &str, name: &str) -> bool {
    scan_rust_attributes(source)
        .bodies
        .into_iter()
        .any(|attribute| attribute_effectively_named(attribute, name))
}

fn regular_file_metadata(path: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_file() && !metadata.file_type().is_symlink() {
        Ok(metadata)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("not a regular file: {}", path.display()),
        ))
    }
}

fn result_mut<'a>(results: &'a mut [DimensionResult], dimension: &str) -> &'a mut DimensionResult {
    results
        .iter_mut()
        .find(|result| result.dimension == dimension)
        .expect("known grading dimension")
}

fn strip_toml_comment(line: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote == Some('"') {
            escaped = true;
            continue;
        }
        match character {
            '\'' | '"' if quote == Some(character) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(character),
            '#' if quote.is_none() => return &line[..index],
            _ => {}
        }
    }
    line
}

/// Replace comments and string/character literal bodies with whitespace while
/// preserving punctuation needed for conservative API checks.
fn sanitize_rust(source: &str) -> String {
    #[derive(Clone, Copy)]
    enum State {
        Code,
        LineComment,
        BlockComment(usize),
        String,
        Character,
        RawString(usize),
    }

    let bytes = source.as_bytes();
    let mut output = String::with_capacity(source.len());
    let mut index = 0usize;
    let mut state = State::Code;
    while index < bytes.len() {
        match state {
            State::Code if bytes[index..].starts_with(b"//") => {
                output.push_str("  ");
                index += 2;
                state = State::LineComment;
            }
            State::Code if bytes[index..].starts_with(b"/*") => {
                output.push_str("  ");
                index += 2;
                state = State::BlockComment(1);
            }
            State::Code if bytes[index] == b'"' => {
                output.push(' ');
                index += 1;
                state = State::String;
            }
            State::Code if bytes[index] == b'\'' && looks_like_character(bytes, index) => {
                output.push(' ');
                index += 1;
                state = State::Character;
            }
            State::Code if bytes[index] == b'r' => {
                if let Some((hashes, consumed)) = raw_string_start(&bytes[index..]) {
                    output.extend(std::iter::repeat_n(' ', consumed));
                    index += consumed;
                    state = State::RawString(hashes);
                } else {
                    output.push('r');
                    index += 1;
                }
            }
            State::Code => {
                output.push(bytes[index] as char);
                index += 1;
            }
            State::LineComment if bytes[index] == b'\n' => {
                output.push('\n');
                index += 1;
                state = State::Code;
            }
            State::LineComment => {
                output.push(' ');
                index += 1;
            }
            State::BlockComment(depth) if bytes[index..].starts_with(b"/*") => {
                output.push_str("  ");
                index += 2;
                state = State::BlockComment(depth + 1);
            }
            State::BlockComment(depth) if bytes[index..].starts_with(b"*/") => {
                output.push_str("  ");
                index += 2;
                state = if depth == 1 {
                    State::Code
                } else {
                    State::BlockComment(depth - 1)
                };
            }
            State::BlockComment(depth) => {
                output.push(if bytes[index] == b'\n' { '\n' } else { ' ' });
                index += 1;
                state = State::BlockComment(depth);
            }
            State::String if bytes[index] == b'\\' => {
                output.push(' ');
                index += 1;
                if index < bytes.len() {
                    output.push(if bytes[index] == b'\n' { '\n' } else { ' ' });
                    index += 1;
                }
            }
            State::String if bytes[index] == b'"' => {
                output.push(' ');
                index += 1;
                state = State::Code;
            }
            State::String => {
                output.push(if bytes[index] == b'\n' { '\n' } else { ' ' });
                index += 1;
            }
            State::Character if bytes[index] == b'\\' => {
                output.push(' ');
                index += 1;
                if index < bytes.len() {
                    output.push(' ');
                    index += 1;
                }
            }
            State::Character if bytes[index] == b'\'' => {
                output.push(' ');
                index += 1;
                state = State::Code;
            }
            State::Character => {
                output.push(if bytes[index] == b'\n' { '\n' } else { ' ' });
                index += 1;
            }
            State::RawString(hashes) => {
                let mut terminator = Vec::with_capacity(hashes + 1);
                terminator.push(b'"');
                terminator.extend(std::iter::repeat_n(b'#', hashes));
                if bytes[index..].starts_with(&terminator) {
                    output.extend(std::iter::repeat_n(' ', terminator.len()));
                    index += terminator.len();
                    state = State::Code;
                } else {
                    output.push(if bytes[index] == b'\n' { '\n' } else { ' ' });
                    index += 1;
                }
            }
        }
    }
    output
}

fn looks_like_character(bytes: &[u8], index: usize) -> bool {
    let Some(tail) = bytes.get(index + 1..) else {
        return false;
    };
    if tail.first() == Some(&b'\\') {
        tail.get(2) == Some(&b'\'') || tail.get(3) == Some(&b'\'')
    } else {
        tail.get(1) == Some(&b'\'')
    }
}

fn raw_string_start(bytes: &[u8]) -> Option<(usize, usize)> {
    if bytes.first() != Some(&b'r') {
        return None;
    }
    let mut hashes = 0usize;
    while bytes.get(1 + hashes) == Some(&b'#') {
        hashes += 1;
    }
    (bytes.get(1 + hashes) == Some(&b'"')).then_some((hashes, hashes + 2))
}

fn rust_identifiers(source: &str) -> impl Iterator<Item = &str> {
    source.split(|character: char| !(character == '_' || character.is_ascii_alphanumeric()))
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character <= '\u{1f}' => {
                let _ = write!(escaped, "\\u{:04x}", character as u32);
            }
            character => escaped.push(character),
        }
    }
    escaped
}

/// Compute a lowercase SHA-256 digest without an external dependency.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize_hex())
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize_hex()
}

struct Sha256 {
    state: [u32; 8],
    length_bytes: u64,
    buffer: [u8; 64],
    buffer_len: usize,
}

impl Sha256 {
    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            length_bytes: 0,
            buffer: [0; 64],
            buffer_len: 0,
        }
    }

    fn update(&mut self, mut bytes: &[u8]) {
        self.length_bytes = self.length_bytes.wrapping_add(bytes.len() as u64);
        if self.buffer_len != 0 {
            let needed = 64 - self.buffer_len;
            let copied = needed.min(bytes.len());
            self.buffer[self.buffer_len..self.buffer_len + copied]
                .copy_from_slice(&bytes[..copied]);
            self.buffer_len += copied;
            bytes = &bytes[copied..];
            if self.buffer_len == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffer_len = 0;
            } else {
                return;
            }
        }
        while bytes.len() >= 64 {
            let block: &[u8; 64] = bytes[..64].try_into().expect("64-byte SHA block");
            self.compress(block);
            bytes = &bytes[64..];
        }
        self.buffer[..bytes.len()].copy_from_slice(bytes);
        self.buffer_len = bytes.len();
    }

    fn finalize_hex(mut self) -> String {
        let bit_length = self.length_bytes.wrapping_mul(8);
        let mut padding = [0u8; 128];
        padding[0] = 0x80;
        let padding_len = if self.buffer_len < 56 {
            56 - self.buffer_len
        } else {
            120 - self.buffer_len
        };
        self.update(&padding[..padding_len]);
        // Do not use `update` for the encoded original bit length when
        // reasoning about the digest length; the compressor only needs bytes.
        self.update(&bit_length.to_be_bytes());

        let mut output = String::with_capacity(64);
        for word in self.state {
            let _ = write!(output, "{word:08x}");
        }
        output
    }

    fn compress(&mut self, block: &[u8; 64]) {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];
        let mut schedule = [0u32; 64];
        for (index, bytes) in block.chunks_exact(4).take(16).enumerate() {
            schedule[index] = u32::from_be_bytes(bytes.try_into().expect("four bytes"));
        }
        for index in 16..64 {
            let s0 = schedule[index - 15].rotate_right(7)
                ^ schedule[index - 15].rotate_right(18)
                ^ (schedule[index - 15] >> 3);
            let s1 = schedule[index - 2].rotate_right(17)
                ^ schedule[index - 2].rotate_right(19)
                ^ (schedule[index - 2] >> 10);
            schedule[index] = schedule[index - 16]
                .wrapping_add(s0)
                .wrapping_add(schedule[index - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(choice)
                .wrapping_add(K[index])
                .wrapping_add(schedule[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (state, value) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *state = state.wrapping_add(value);
        }
    }
}

/// Verify a sorted `SHA256  relative/path` artifact manifest against `root`.
pub fn verify_artifact_manifest(root: &Path, manifest: &str) -> io::Result<Vec<String>> {
    let root_metadata = fs::symlink_metadata(root)?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "artifact root must be a real directory",
        ));
    }
    let mut failures = Vec::new();
    let mut previous = None::<String>;
    let mut artifact_count = 0usize;
    for (line_index, line) in manifest.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let Some((expected, relative)) = line.split_once("  ") else {
            failures.push(format!("manifest line {} is malformed", line_index + 1));
            continue;
        };
        artifact_count += 1;
        if !is_sha256(expected) {
            failures.push(format!(
                "manifest line {} has invalid SHA-256",
                line_index + 1
            ));
            continue;
        }
        if !safe_relative_path(relative) {
            failures.push(format!("manifest line {} has unsafe path", line_index + 1));
            continue;
        }
        if previous.as_deref().is_some_and(|value| value >= relative) {
            failures.push(format!(
                "manifest line {} is not strictly sorted",
                line_index + 1
            ));
        }
        previous = Some(relative.to_owned());
        let path = root.join(relative);
        match verify_regular_path_beneath(root, relative).and_then(|_| sha256_file(&path)) {
            Ok(actual) if actual == expected => {}
            Ok(_) => failures.push(format!("artifact hash mismatch: {relative}")),
            Err(error) => failures.push(format!("artifact unavailable: {relative}: {error}")),
        }
    }
    if artifact_count == 0 {
        failures.push("manifest contains no artifacts".to_owned());
    }
    failures.sort();
    failures.dedup();
    Ok(failures)
}

/// Verify the `s114-command-journal-v1` hash chain emitted by the fixed runner.
///
/// The first line must be the twelve-field header. Records start at sequence
/// one with an all-zero previous hash. Each entry hash covers its first eleven
/// tab-separated fields, without a trailing newline.
pub fn verify_journal_chain(journal: &str) -> Vec<String> {
    const SCHEMA: &str = "s114-command-journal-v1";
    const HEADER: &str = "schema\tsequence\tprevious_sha256\tstage_b64\tcwd_b64\targv_b64\t\
        exit_code\tstdout_path_b64\tstdout_sha256\tstderr_path_b64\tstderr_sha256\tentry_sha256";
    const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    let mut failures = Vec::new();
    let mut lines = journal.lines();
    if lines.next() != Some(HEADER) {
        failures.push("journal header mismatch".to_owned());
        return failures;
    }
    let mut expected_sequence = 1usize;
    let mut expected_previous = ZERO_HASH.to_owned();
    let mut record_count = 0usize;
    for (record_index, line) in lines.enumerate() {
        let line_index = record_index + 2;
        if line.trim().is_empty() {
            failures.push(format!("journal line {line_index} is empty"));
            continue;
        }
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 12 {
            failures.push(format!("journal line {line_index} is malformed"));
            continue;
        }
        record_count += 1;
        if fields[0] != SCHEMA {
            failures.push(format!("journal line {line_index} has wrong schema"));
        }
        if fields[1].parse::<usize>().ok() != Some(expected_sequence) {
            failures.push(format!("journal line {line_index} has wrong sequence"));
        }
        if fields[2] != expected_previous {
            failures.push(format!("journal line {line_index} breaks previous hash"));
        }
        for field_index in [3usize, 4, 5, 7, 9] {
            if !is_base64_csv_or_value(fields[field_index], field_index == 5) {
                failures.push(format!(
                    "journal line {line_index} has invalid base64 field {}",
                    field_index + 1
                ));
            }
        }
        if fields[6].parse::<u32>().is_err() {
            failures.push(format!("journal line {line_index} has invalid exit code"));
        }
        if !is_sha256(fields[8]) || !is_sha256(fields[10]) {
            failures.push(format!("journal line {line_index} has invalid output hash"));
        }
        let canonical = fields[..11].join("\t");
        let actual = sha256_bytes(canonical.as_bytes());
        if fields[11] != actual {
            failures.push(format!("journal line {line_index} has wrong entry hash"));
        }
        expected_sequence += 1;
        expected_previous = fields[11].to_owned();
    }
    if record_count == 0 {
        failures.push("journal contains no command records".to_owned());
    }
    failures.sort();
    failures.dedup();
    failures
}

fn is_base64_csv_or_value(value: &str, comma_separated: bool) -> bool {
    if comma_separated {
        !value.is_empty()
            && value
                .split(',')
                .all(|part| part.is_empty() || is_base64_value(part))
    } else {
        is_base64_value(value)
    }
}

fn verify_regular_path_beneath(root: &Path, relative: &str) -> io::Result<()> {
    let mut path = root.to_path_buf();
    for component in Path::new(relative).components() {
        let Component::Normal(component) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "artifact path is not a safe relative path",
            ));
        };
        path.push(component);
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("artifact path traverses symlink: {}", path.display()),
            ));
        }
    }
    regular_file_metadata(&path).map(|_| ())
}

fn is_base64_value(value: &str) -> bool {
    if value.is_empty() || !value.len().is_multiple_of(4) {
        return false;
    }
    let padding = value.bytes().rev().take_while(|byte| *byte == b'=').count();
    padding <= 2
        && value[..value.len() - padding]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/')
        && value[value.len() - padding..]
            .bytes()
            .all(|byte| byte == b'=')
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn safe_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && !path.is_absolute()
        && path.components().all(|component| {
            matches!(component, Component::Normal(_))
                && component.as_os_str() != "."
                && component.as_os_str() != ".."
        })
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn sha256_matches_standard_vectors() {
        assert_eq!(
            sha256_bytes(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn rust_sanitizer_hides_prose_but_keeps_code() {
        let source = r###"
            // unsafe std::net
            const WORDS: &str = "unsafe Command::new";
            const RAW: &str = r#"TcpStream"#;
            unsafe fn rejected() {}
        "###;
        let sanitized = sanitize_rust(source);
        assert!(!sanitized.contains("TcpStream"));
        assert!(!sanitized.contains("Command::new"));
        assert!(rust_identifiers(&sanitized).any(|identifier| identifier == "unsafe"));
    }

    #[test]
    fn attribute_scanner_distinguishes_attributes_from_identifiers() {
        let ordinary = sanitize_rust(
            "fn cfg() {} fn ignore() {} fn demo() { let cfg = cfg!(test); \
             let ignore = cfg; assert!(ignore); }",
        );
        assert!(scan_rust_attributes(&ordinary).bodies.is_empty());

        for allowed in ["cfg(test)", "allow(ignore)", "cfg_attr(test, should_panic)"] {
            assert!(!test_attribute_is_disabled(allowed), "{allowed}");
        }
        for rejected in [
            "ignore",
            "ignore = \"reason\"",
            "cfg(any())",
            "cfg(not(test))",
            "cfg_attr(test, ignore)",
            "cfg_attr(test, cfg(any()))",
        ] {
            let rejected = sanitize_rust(rejected);
            assert!(test_attribute_is_disabled(&rejected), "{rejected}");
        }

        let nested_path = sanitize_rust("#[cfg_attr(test, path = \"elsewhere.rs\")] mod hidden;");
        assert!(contains_attribute_named(&nested_path, "path"));
    }

    #[test]
    fn dimension_details_are_bounded_for_the_output_envelope() {
        let mut dimensions: Vec<_> = DIMENSIONS
            .iter()
            .copied()
            .map(DimensionResult::new)
            .collect();
        for result in &mut dimensions {
            for prefix in ['a', 'b'] {
                result.fail(format!("{prefix}{}", "\u{1}".repeat(256)));
            }
            for index in 0..10_000 {
                result.fail(format!("violation-{index:05}"));
            }
            result.finish();
            assert_eq!(result.details.len(), MAX_RETAINED_DETAILS + 1);
            assert!(
                result
                    .details
                    .iter()
                    .any(|detail| detail == "additional_violations: 10000")
            );
        }
        let jsonl = GradeReport { dimensions }.to_jsonl();
        assert_eq!(jsonl.lines().count(), DIMENSIONS.len());
        assert!(
            jsonl.len() < 8 * 1024,
            "bounded JSONL was {} bytes",
            jsonl.len()
        );
    }

    #[test]
    fn test_body_extractor_balances_nested_code_and_requires_real_oracles() {
        let source = r###"
            #[test]
            fn nested_assertion() {
                let fake = "} assert!(false) {";
                /* } panic!() { */
                if true { assert_eq!({ 1 }, 1); }
            }

            #[test]
            fn result_question_mark() -> Result<(), ExampleError> {
                operation()?;
                Ok(())
            }

            #[test]
            fn empty_despite_fake_text() {
                let fake = "assert!(true)";
                // panic!("not an oracle");
            }

            #[test]
            fn standalone_weak_predicates() {
                matches!(value, Some(_));
                value.is_err();
            }

            #[test]
            fn custom_assertion_macro() {
                assert_contract!(value);
            }

            #[test]
            fn divergent_placeholder() {
                todo!();
            }

            #[test]
            fn exact_failing_result_methods() {
                operation().unwrap();
                operation().unwrap_err();
                operation().expect("message");
                operation().expect_err("message");
            }

            #[test]
            fn fallback_result_methods() {
                operation().unwrap_or_default();
                operation().unwrap_or_else(recover);
                operation().expectation();
            }
        "###;
        let sanitized = sanitize_rust(source);
        let extracted = extract_test_functions(&sanitized);
        assert_eq!(extracted.malformed_attributes, 0);
        assert_eq!(extracted.functions.len(), 8);
        assert_eq!(extracted.functions[0].name, "nested_assertion");
        assert!(test_body_has_oracle(&extracted.functions[0]));
        assert!(test_body_has_oracle(&extracted.functions[1]));
        assert!(!test_body_has_oracle(&extracted.functions[2]));
        assert!(!test_body_has_oracle(&extracted.functions[3]));
        assert!(!test_body_has_oracle(&extracted.functions[4]));
        assert!(!test_body_has_oracle(&extracted.functions[5]));
        assert!(test_body_has_oracle(&extracted.functions[6]));
        assert!(!test_body_has_oracle(&extracted.functions[7]));
    }

    #[test]
    fn topical_api_oracles_require_absolute_coupled_calls() {
        let assertion =
            sanitize_rust("assert_eq!(::release_plan::parse_manifest(input), expected);");
        assert!(contains_coupled_api_oracle(&assertion, "parse_manifest"));

        let result_method =
            sanitize_rust("let jobs = ::release_plan::parse_manifest(input).unwrap();");
        assert!(contains_coupled_api_oracle(
            &result_method,
            "parse_manifest"
        ));

        let local_shadow = sanitize_rust("parse_manifest(input).unwrap(); assert!(true);");
        assert!(!contains_coupled_api_oracle(
            &local_shadow,
            "parse_manifest"
        ));

        let unrelated = sanitize_rust(
            "let _ = ::release_plan::parse_manifest(input); Fake.expect(\"ignored\");",
        );
        assert!(!contains_coupled_api_oracle(&unrelated, "parse_manifest"));

        let second_macro_argument =
            sanitize_rust("assert!(true, \"{:?}\", ::release_plan::parse_manifest(input));");
        assert!(!contains_coupled_api_oracle(
            &second_macro_argument,
            "parse_manifest"
        ));

        assert!(contains_oracle_alias(&sanitize_rust(
            "use std::println as assert;"
        )));
        assert!(contains_oracle_alias(&sanitize_rust(
            "pub use std::println as r#assert_eq;"
        )));
    }
}

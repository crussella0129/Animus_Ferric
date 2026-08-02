//! Deterministic, metadata-only repository briefs for benchmark policy A/Bs.
//!
//! The brief deliberately contains relative paths and classifications only. It
//! never reads file contents, resolves symlink targets, or prints the workspace
//! root. This keeps it useful as bounded repository context without turning it
//! into a second source of secrets or machine identity.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::Path;

/// Hard limits for a generated repository brief.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepositoryBriefLimits {
    /// Maximum number of visible file-like entries retained by the scan.
    pub max_files: usize,
    /// Maximum UTF-8 bytes returned in [`RepositoryBrief::text`].
    pub max_bytes: usize,
}

impl Default for RepositoryBriefLimits {
    fn default() -> Self {
        Self {
            max_files: 256,
            max_bytes: 16 * 1024,
        }
    }
}

/// A deterministic repository summary suitable for prepending to a benchmark
/// prompt as an opt-in policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryBrief {
    /// Versioned, metadata-only text. Its length never exceeds `max_bytes`.
    pub text: String,
    /// Visible file-like entries retained before text rendering.
    pub files_included: usize,
    /// Entries that fit in the rendered text as complete lines.
    pub files_rendered: usize,
    /// Retained files classified as agent or human instructions.
    pub instruction_files: usize,
    /// Retained files classified as workspace or package manifests.
    pub manifest_files: usize,
    /// Non-excluded directories visited, including the workspace root.
    pub directories_visited: usize,
    /// Hidden entries excluded without inspecting their contents.
    pub hidden_entries_omitted: usize,
    /// Generated/runtime directories excluded without descending into them.
    pub runtime_directories_omitted: usize,
    /// Secret-looking files or directories excluded without inspecting them.
    pub sensitive_entries_omitted: usize,
    /// The scan found more visible files than `max_files` allowed.
    pub truncated_by_file_limit: bool,
    /// The complete metadata rendering exceeded `max_bytes`.
    pub truncated_by_byte_limit: bool,
}

impl RepositoryBrief {
    /// Whether either configured limit truncated this brief.
    pub fn truncated(&self) -> bool {
        self.truncated_by_file_limit || self.truncated_by_byte_limit
    }
}

/// Generate a stable, bounded, metadata-only brief of `workspace_root`.
///
/// Directory entries are sorted before traversal, symlinks are listed but are
/// never followed, and all emitted paths are relative with `/` separators.
/// Hidden entries, common runtime/generated directories, and secret-looking
/// paths are omitted. An invalid or unreadable workspace returns an I/O error
/// instead of silently producing a misleading brief.
pub fn generate_repository_brief(
    workspace_root: &Path,
    limits: RepositoryBriefLimits,
) -> io::Result<RepositoryBrief> {
    let metadata = fs::metadata(workspace_root)?;
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "repository brief root must be a directory",
        ));
    }

    let mut scan = Scan::new(limits.max_files);
    scan.visit_directory(workspace_root, Path::new(""))?;

    let instruction_files = scan
        .entries
        .iter()
        .filter(|entry| entry.relevance == Relevance::Instruction)
        .count();
    let manifest_files = scan
        .entries
        .iter()
        .filter(|entry| entry.relevance == Relevance::Manifest)
        .count();

    let untruncated_header = render_header(limits, scan.file_limit_reached, false);
    let complete_length = scan
        .entries
        .iter()
        .fold(untruncated_header.len(), |length, entry| {
            length.saturating_add(render_entry(entry).len())
        });
    let byte_limit_reached = complete_length > limits.max_bytes;
    let header = render_header(limits, scan.file_limit_reached, byte_limit_reached);
    let (text, files_rendered) = render_bounded(header, &scan.entries, limits.max_bytes);

    Ok(RepositoryBrief {
        text,
        files_included: scan.entries.len(),
        files_rendered,
        instruction_files,
        manifest_files,
        directories_visited: scan.directories_visited,
        hidden_entries_omitted: scan.hidden_entries_omitted,
        runtime_directories_omitted: scan.runtime_directories_omitted,
        sensitive_entries_omitted: scan.sensitive_entries_omitted,
        truncated_by_file_limit: scan.file_limit_reached,
        truncated_by_byte_limit: byte_limit_reached,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Relevance {
    Ordinary,
    Instruction,
    Manifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BriefEntry {
    path: String,
    relevance: Relevance,
    symlink: bool,
}

struct Scan {
    max_files: usize,
    entries: Vec<BriefEntry>,
    directories_visited: usize,
    hidden_entries_omitted: usize,
    runtime_directories_omitted: usize,
    sensitive_entries_omitted: usize,
    file_limit_reached: bool,
}

impl Scan {
    fn new(max_files: usize) -> Self {
        Self {
            max_files,
            entries: Vec::with_capacity(max_files.min(256)),
            directories_visited: 0,
            hidden_entries_omitted: 0,
            runtime_directories_omitted: 0,
            sensitive_entries_omitted: 0,
            file_limit_reached: false,
        }
    }

    /// Returns `true` when traversal should stop because another visible file
    /// was found after the configured file budget was exhausted.
    fn visit_directory(&mut self, absolute: &Path, relative: &Path) -> io::Result<bool> {
        self.directories_visited = self.directories_visited.saturating_add(1);

        let mut entries = fs::read_dir(absolute)?.collect::<io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries {
            let name = entry.file_name();
            let name_text = name.to_string_lossy();
            let file_type = entry.file_type()?;

            if file_type.is_dir() {
                if is_runtime_directory(&name) {
                    self.runtime_directories_omitted =
                        self.runtime_directories_omitted.saturating_add(1);
                    continue;
                }
                if is_hidden(&name_text) {
                    self.hidden_entries_omitted = self.hidden_entries_omitted.saturating_add(1);
                    continue;
                }
                if is_sensitive_directory(&name_text) {
                    self.sensitive_entries_omitted =
                        self.sensitive_entries_omitted.saturating_add(1);
                    continue;
                }

                let child_relative = relative.join(&name);
                if self.visit_directory(&entry.path(), &child_relative)? {
                    return Ok(true);
                }
                continue;
            }

            if is_hidden(&name_text) {
                self.hidden_entries_omitted = self.hidden_entries_omitted.saturating_add(1);
                continue;
            }
            if is_sensitive_file(&name_text) {
                self.sensitive_entries_omitted = self.sensitive_entries_omitted.saturating_add(1);
                continue;
            }
            if self.entries.len() == self.max_files {
                self.file_limit_reached = true;
                return Ok(true);
            }

            let child_relative = relative.join(&name);
            self.entries.push(BriefEntry {
                path: display_relative_path(&child_relative),
                relevance: classify_file(&name_text),
                symlink: file_type.is_symlink(),
            });
        }

        Ok(false)
    }
}

fn render_header(
    limits: RepositoryBriefLimits,
    file_truncated: bool,
    byte_truncated: bool,
) -> String {
    let truncation = match (file_truncated, byte_truncated) {
        (false, false) => "none",
        (true, false) => "file-limit",
        (false, true) => "byte-limit",
        (true, true) => "file-limit,byte-limit",
    };
    format!(
        "repository-brief/v1 truncation={truncation}\n\
         limits files={} bytes={}\n\
         privacy=relative-path-metadata-only; contents=excluded; symlinks=not-followed\n\
         tree:\n",
        limits.max_files, limits.max_bytes
    )
}

fn render_entry(entry: &BriefEntry) -> String {
    let annotation = match (entry.relevance, entry.symlink) {
        (Relevance::Instruction, false) => " [instruction]",
        (Relevance::Manifest, false) => " [manifest]",
        (Relevance::Ordinary, false) => "",
        (Relevance::Instruction, true) => " [instruction,symlink]",
        (Relevance::Manifest, true) => " [manifest,symlink]",
        (Relevance::Ordinary, true) => " [symlink]",
    };
    format!("- {}{annotation}\n", entry.path)
}

fn render_bounded(mut text: String, entries: &[BriefEntry], max_bytes: usize) -> (String, usize) {
    if text.len() > max_bytes {
        truncate_utf8(&mut text, max_bytes);
        return (text, 0);
    }

    let mut rendered = 0;
    for entry in entries {
        let line = render_entry(entry);
        if text.len().saturating_add(line.len()) > max_bytes {
            break;
        }
        text.push_str(&line);
        rendered += 1;
    }
    (text, rendered)
}

fn truncate_utf8(text: &mut String, max_bytes: usize) {
    let mut boundary = max_bytes.min(text.len());
    while !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    text.truncate(boundary);
}

fn display_relative_path(path: &Path) -> String {
    path.components()
        .map(|component| escape_component(&component.as_os_str().to_string_lossy()))
        .collect::<Vec<_>>()
        .join("/")
}

fn escape_component(component: &str) -> String {
    component
        .chars()
        .flat_map(|character| {
            if character == '\\' || character.is_control() {
                character.escape_default().collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .collect()
}

fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

fn is_runtime_directory(name: &OsStr) -> bool {
    let lower = name.to_string_lossy().to_ascii_lowercase();
    matches!(
        lower.as_str(),
        ".git"
            | ".ferric"
            | "target"
            | "node_modules"
            | "__pycache__"
            | ".pytest_cache"
            | ".ruff_cache"
            | ".mypy_cache"
            | ".tox"
            | ".nox"
            | ".venv"
            | "venv"
            | "dist"
            | "coverage"
    )
}

fn is_sensitive_directory(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "secrets" | "credentials" | "private-keys" | "private_keys"
    )
}

fn is_sensitive_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let path = Path::new(&lower);
    let extension = path.extension().and_then(OsStr::to_str).unwrap_or_default();

    matches!(extension, "key" | "pem" | "p12" | "pfx" | "kdbx" | "jks")
        || matches!(
            lower.as_str(),
            "credentials"
                | "credentials.json"
                | "auth.json"
                | "id_rsa"
                | "id_ed25519"
                | "known_hosts"
                | "secrets.json"
                | "secrets.toml"
                | "secrets.yaml"
                | "secrets.yml"
        )
        || lower.starts_with("secret.")
        || lower.ends_with("-credentials.json")
        || lower.ends_with("_credentials.json")
}

fn classify_file(name: &str) -> Relevance {
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "agents.md" | "claude.md" | "codex.md" | "contributing.md"
    ) || lower == "readme"
        || lower.starts_with("readme.")
    {
        return Relevance::Instruction;
    }

    if matches!(
        lower.as_str(),
        "cargo.toml"
            | "cargo.lock"
            | "package.json"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "pyproject.toml"
            | "poetry.lock"
            | "uv.lock"
            | "go.mod"
            | "go.sum"
            | "go.work"
            | "composer.json"
            | "composer.lock"
            | "pom.xml"
            | "build.gradle"
            | "build.gradle.kts"
            | "settings.gradle"
            | "settings.gradle.kts"
            | "gradle.properties"
            | "gemfile"
            | "gemfile.lock"
            | "mix.exs"
            | "mix.lock"
            | "deno.json"
            | "deno.jsonc"
            | "deno.lock"
            | "rust-toolchain"
            | "rust-toolchain.toml"
            | "makefile"
            | "justfile"
            | "cmakelists.txt"
            | "dockerfile"
    ) || (lower.starts_with("requirements") && lower.ends_with(".txt"))
        || (lower.starts_with("docker-compose.")
            && (lower.ends_with(".yaml") || lower.ends_with(".yml")))
    {
        Relevance::Manifest
    } else {
        Relevance::Ordinary
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static FIXTURE_ID: AtomicUsize = AtomicUsize::new(0);

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "ferric-repository-brief-test-{}-{id}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("create isolated fixture root");
            Self { root }
        }

        fn file(&self, relative: &str, contents: &str) {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create fixture parent");
            }
            fs::write(path, contents).expect("write fixture file");
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).expect("remove isolated fixture root");
        }
    }

    #[test]
    fn brief_is_stable_relative_and_metadata_only() {
        let fixture = Fixture::new();
        fixture.file("zeta.rs", "FILE-CONTENT-MUST-NOT-LEAK");
        fixture.file("src/z.rs", "z");
        fixture.file("src/a.rs", "a");
        fixture.file("AGENTS.md", "INSTRUCTION-CONTENT-MUST-NOT-LEAK");
        fixture.file("Cargo.toml", "MANIFEST-CONTENT-MUST-NOT-LEAK");

        let limits = RepositoryBriefLimits {
            max_files: 20,
            max_bytes: 4096,
        };
        let first = generate_repository_brief(&fixture.root, limits).unwrap();
        let second = generate_repository_brief(&fixture.root, limits).unwrap();

        assert_eq!(first, second);
        assert!(!first.truncated());
        assert!(first.text.contains("- AGENTS.md [instruction]\n"));
        assert!(first.text.contains("- Cargo.toml [manifest]\n"));
        assert!(first.text.contains("- src/a.rs\n"));
        assert!(first.text.find("src/a.rs").unwrap() < first.text.find("src/z.rs").unwrap());
        assert!(!first.text.contains("MUST-NOT-LEAK"));
        assert!(
            !first
                .text
                .contains(&fixture.root.to_string_lossy().to_string())
        );
        assert!(!first.text.contains('\\'));
        assert_eq!(first.instruction_files, 1);
        assert_eq!(first.manifest_files, 1);
    }

    #[test]
    fn runtime_hidden_and_sensitive_paths_are_not_disclosed() {
        let fixture = Fixture::new();
        fixture.file("src/lib.rs", "safe");
        fixture.file(".git/config", "machine identity");
        fixture.file(".ferric/trace/run.jsonl", "prompt material");
        fixture.file("target/debug/ferric", "binary");
        fixture.file(".hidden/note.txt", "hidden");
        fixture.file("secrets/production.txt", "secret");
        fixture.file("deploy/private.pem", "secret");
        fixture.file("credentials.json", "secret");

        let brief = generate_repository_brief(
            &fixture.root,
            RepositoryBriefLimits {
                max_files: 20,
                max_bytes: 4096,
            },
        )
        .unwrap();

        assert!(brief.text.contains("src/lib.rs"));
        for excluded in [
            ".git",
            ".ferric",
            "target",
            ".hidden",
            "secrets",
            "private.pem",
            "credentials.json",
        ] {
            assert!(!brief.text.contains(excluded), "leaked {excluded}");
        }
        assert_eq!(brief.runtime_directories_omitted, 3);
        assert_eq!(brief.hidden_entries_omitted, 1);
        assert_eq!(brief.sensitive_entries_omitted, 3);
    }

    #[test]
    fn file_limit_stops_at_a_deterministic_prefix_and_reports_it() {
        let fixture = Fixture::new();
        fixture.file("a.txt", "a");
        fixture.file("b.txt", "b");
        fixture.file("c.txt", "c");

        let brief = generate_repository_brief(
            &fixture.root,
            RepositoryBriefLimits {
                max_files: 2,
                max_bytes: 4096,
            },
        )
        .unwrap();

        assert_eq!(brief.files_included, 2);
        assert!(brief.truncated_by_file_limit);
        assert!(!brief.truncated_by_byte_limit);
        assert!(
            brief
                .text
                .starts_with("repository-brief/v1 truncation=file-limit\n")
        );
        assert!(brief.text.contains("a.txt"));
        assert!(brief.text.contains("b.txt"));
        assert!(!brief.text.contains("c.txt"));
    }

    #[test]
    fn byte_limit_keeps_utf8_valid_and_reports_it_when_space_allows() {
        let fixture = Fixture::new();
        fixture.file("src/alpha.rs", "a");
        fixture.file("src/bravo-🦀.rs", "b");
        fixture.file("src/charlie.rs", "c");

        let limits = RepositoryBriefLimits {
            max_files: 20,
            max_bytes: 180,
        };
        let brief = generate_repository_brief(&fixture.root, limits).unwrap();

        assert!(brief.truncated_by_byte_limit);
        assert!(brief.truncated());
        assert!(brief.text.len() <= limits.max_bytes);
        assert!(
            brief
                .text
                .starts_with("repository-brief/v1 truncation=byte-limit\n")
        );
        assert!(brief.files_rendered < brief.files_included);
    }

    #[test]
    fn zero_limits_are_safe_and_report_through_structured_metadata() {
        let fixture = Fixture::new();
        fixture.file("file.txt", "content");

        let brief = generate_repository_brief(
            &fixture.root,
            RepositoryBriefLimits {
                max_files: 0,
                max_bytes: 0,
            },
        )
        .unwrap();

        assert!(brief.text.is_empty());
        assert_eq!(brief.files_included, 0);
        assert!(brief.truncated_by_file_limit);
        assert!(brief.truncated_by_byte_limit);
    }

    #[test]
    fn non_directory_root_is_rejected() {
        let fixture = Fixture::new();
        fixture.file("file.txt", "content");

        let error = generate_repository_brief(
            &fixture.root.join("file.txt"),
            RepositoryBriefLimits::default(),
        )
        .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}

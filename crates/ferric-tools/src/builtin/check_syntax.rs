use std::path::Path;
use std::process::Command;

use crate::control::{SyntaxState, SyntaxTransition, SyntaxUncheckedReason, sha256_bytes};
use rustpython_parser::{Parse, ast};

/// Best-effort syntax check for recognized file extensions. Returns `None` if
/// the file is clean or not recognized; `Some(warning)` if a syntax issue is
/// detected. Non-blocking: a missing interpreter is a silent no-op (returns
/// `None`), never an error. The check runs the WRITTEN content (already on
/// disk), not the in-memory string, so it catches real-world encoding issues.
pub fn check_syntax(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "py" => check_python(path),
        // Future: "rs", "js", "ts"
        _ => None,
    }
}

/// Python: `python -c "import py_compile; py_compile.compile('<path>', doraise=True)"`.
/// Falls back to `python3` if `python` is absent. A missing interpreter is a
/// silent no-op (the user might not have Python installed; the agent is still
/// correct to write the file).
fn check_python(path: &Path) -> Option<String> {
    let path_str = path.to_str()?;
    // Escape single quotes and backslashes for the python string literal.
    let escaped = path_str.replace('\\', "\\\\").replace('\'', "\\'");
    let check_code = format!(
        "import py_compile; py_compile.compile('{}', doraise=True)",
        escaped
    );

    for interpreter in &["python", "python3"] {
        match Command::new(interpreter)
            .arg("-c")
            .arg(&check_code)
            .output()
        {
            Ok(output) if !output.status.success() => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                // Extract just the last line (the actual error), not the full traceback.
                let error_line = stderr
                    .lines()
                    .rfind(|l| !l.trim().is_empty())
                    .unwrap_or("syntax error");
                return Some(format!("syntax check: {}", error_line.trim()));
            }
            Ok(_) => return None, // clean
            Err(_) => continue,   // interpreter not found, try next
        }
    }
    None // no interpreter available — silent no-op
}

/// Parse exact in-memory candidate bytes without writing a temporary file,
/// importing candidate code, starting a process, or consulting `PATH`.
pub(crate) fn candidate_syntax_transition(
    path: &Path,
    before: Option<&[u8]>,
    candidate: &[u8],
) -> SyntaxTransition {
    if path.extension().and_then(|extension| extension.to_str()) != Some("py") {
        return SyntaxTransition {
            before: before.map_or(SyntaxState::Absent, |_| {
                SyntaxState::Unchecked(SyntaxUncheckedReason::UnsupportedExtension)
            }),
            candidate: SyntaxState::Unchecked(SyntaxUncheckedReason::UnsupportedExtension),
            diagnostic_sha256: None,
            warning: None,
        };
    }

    let before_result = before.map(parse_python_bytes);
    let candidate_result = parse_python_bytes(candidate);
    let before_state = before_result
        .as_ref()
        .map_or(SyntaxState::Absent, |result| result.state);
    let candidate_state = candidate_result.state;
    let diagnostic_sha256 = candidate_result
        .diagnostic
        .as_deref()
        .map(|diagnostic| sha256_bytes(diagnostic.as_bytes()));
    let warning = matches!(before_state, SyntaxState::Invalid)
        .then_some(())
        .filter(|_| matches!(candidate_state, SyntaxState::Invalid))
        .map(|_| {
            format!(
                "syntax check: candidate remains invalid while repairing an already-invalid file: {}",
                candidate_result
                    .diagnostic
                    .as_deref()
                    .unwrap_or("syntax error")
            )
        });

    SyntaxTransition {
        before: before_state,
        candidate: candidate_state,
        diagnostic_sha256,
        warning,
    }
}

struct SyntaxParseResult {
    state: SyntaxState,
    diagnostic: Option<String>,
}

fn parse_python_bytes(bytes: &[u8]) -> SyntaxParseResult {
    const MAX_CONTROLLED_PYTHON_BYTES: usize = 2 * 1024 * 1024;
    if bytes.len() > MAX_CONTROLLED_PYTHON_BYTES {
        return SyntaxParseResult {
            state: SyntaxState::Unchecked(SyntaxUncheckedReason::InputTooLarge),
            diagnostic: None,
        };
    }
    let source = match std::str::from_utf8(bytes) {
        Ok(source) => source,
        Err(error) => {
            return SyntaxParseResult {
                state: SyntaxState::Invalid,
                diagnostic: Some(format!(
                    "source is not UTF-8 at byte {}",
                    error.valid_up_to()
                )),
            };
        }
    };
    match ast::Suite::parse(source, "<ferric-candidate>") {
        Ok(_) => SyntaxParseResult {
            state: SyntaxState::Valid,
            diagnostic: None,
        },
        Err(error) => {
            let diagnostic: String = error.to_string().chars().take(512).collect();
            SyntaxParseResult {
                state: SyntaxState::Invalid,
                diagnostic: Some(diagnostic),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::SyntaxUncheckedReason;
    use std::fs;

    #[test]
    fn clean_python_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clean.py");
        fs::write(&path, "def hello():\n    return 42\n").unwrap();
        assert!(check_syntax(&path).is_none());
    }

    #[test]
    fn broken_python_returns_warning() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.py");
        fs::write(&path, "def fibonacci(6):\n    pass\n").unwrap();
        let result = check_syntax(&path);
        // Skip if no Python interpreter is available.
        if let Some(warning) = result {
            assert!(
                warning.contains("syntax"),
                "expected a syntax error, got: {warning}"
            );
        }
    }

    #[test]
    fn unrecognized_extension_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.csv");
        fs::write(&path, "not,code,at,all").unwrap();
        assert!(check_syntax(&path).is_none());
    }

    #[test]
    fn no_extension_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Makefile");
        fs::write(&path, "broken syntax {{{{").unwrap();
        assert!(check_syntax(&path).is_none());
    }

    #[test]
    fn controlled_parser_bounds_source_size_without_starting_a_process() {
        let oversized = vec![b'x'; 2 * 1024 * 1024 + 1];
        let transition = candidate_syntax_transition(Path::new("candidate.py"), None, &oversized);
        assert_eq!(transition.before, SyntaxState::Absent);
        assert_eq!(
            transition.candidate,
            SyntaxState::Unchecked(SyntaxUncheckedReason::InputTooLarge)
        );
        assert!(!transition.blocks_mutation());
        assert!(transition.diagnostic_sha256.is_none());
    }

    #[test]
    fn controlled_parser_treats_invalid_utf8_as_invalid_source() {
        let transition =
            candidate_syntax_transition(Path::new("candidate.py"), None, b"value = '\xff'\n");
        assert_eq!(transition.candidate, SyntaxState::Invalid);
        assert!(transition.diagnostic_sha256.is_some());
    }

    #[test]
    fn syntax_regression_matrix_blocks_only_absent_or_valid_to_invalid() {
        for before in [SyntaxState::Absent, SyntaxState::Valid] {
            let transition = SyntaxTransition {
                before,
                candidate: SyntaxState::Invalid,
                diagnostic_sha256: Some("digest".to_string()),
                warning: None,
            };
            assert!(transition.blocks_mutation());
        }
        for (before, candidate) in [
            (SyntaxState::Invalid, SyntaxState::Invalid),
            (SyntaxState::Invalid, SyntaxState::Valid),
            (SyntaxState::Valid, SyntaxState::Valid),
            (
                SyntaxState::Unchecked(SyntaxUncheckedReason::InputTooLarge),
                SyntaxState::Unchecked(SyntaxUncheckedReason::InputTooLarge),
            ),
        ] {
            let transition = SyntaxTransition {
                before,
                candidate,
                diagnostic_sha256: None,
                warning: None,
            };
            assert!(!transition.blocks_mutation());
        }
    }

    #[test]
    fn candidate_parser_creates_no_temp_or_pycache_files() {
        let directory = tempfile::tempdir().unwrap();
        let logical_path = directory.path().join("candidate.py");
        let _ = candidate_syntax_transition(&logical_path, None, b"value = 1\n");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn controlled_diagnostic_fingerprint_is_path_independent_and_lowercase() {
        let first = candidate_syntax_transition(Path::new("one/location.py"), None, b"def bad(:\n");
        let second =
            candidate_syntax_transition(Path::new("another/location.py"), None, b"def bad(:\n");
        let (Some(first), Some(second)) = (first.diagnostic_sha256, second.diagnostic_sha256)
        else {
            panic!("in-process parser must report deterministic invalid syntax")
        };
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        assert!(
            first
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
    }
}

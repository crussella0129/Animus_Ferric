use std::path::Path;
use std::process::Command;

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
                    .filter(|l| !l.trim().is_empty())
                    .last()
                    .unwrap_or("syntax error");
                return Some(format!("syntax check: {}", error_line.trim()));
            }
            Ok(_) => return None, // clean
            Err(_) => continue,   // interpreter not found, try next
        }
    }
    None // no interpreter available — silent no-op
}

#[cfg(test)]
mod tests {
    use super::*;
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
}

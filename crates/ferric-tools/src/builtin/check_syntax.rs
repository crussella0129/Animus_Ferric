use std::{panic::AssertUnwindSafe, path::Path};

use crate::control::{SyntaxState, SyntaxTransition, SyntaxUncheckedReason, sha256_bytes};
use rustpython_compiler::{
    CompileError, CompileOpts, Mode, ast, codegen::error::CodegenErrorType, parser,
};

const CANDIDATE_SOURCE_PATH: &str = "<ferric-candidate>";
const PYTHON_ADMISSION_COMPILER: &str = "rustpython-compiler/0.5";

/// Return the legacy warning for recognized candidate bytes without reading a
/// file, starting an interpreter, consulting `PATH`, or importing workspace
/// code. Legacy publication remains warning-only; Evidence admission uses the
/// richer transition below.
pub(crate) fn legacy_syntax_warning(path: &Path, candidate: &[u8]) -> Option<String> {
    if path.extension().and_then(|extension| extension.to_str()) != Some("py") {
        return None;
    }
    let compiled = compile_python_bytes(candidate);
    matches!(compiled.state, SyntaxState::Invalid).then(|| {
        format!(
            "syntax check: {}",
            compiled.diagnostic.as_deref().unwrap_or("syntax error")
        )
    })
}

/// Compile exact in-memory candidate bytes without writing a temporary file,
/// importing or executing candidate code, starting a process, or consulting
/// `PATH`.
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

    let before_result = before.map(compile_python_bytes);
    let candidate_result = compile_python_bytes(candidate);
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

struct SyntaxCompileResult {
    state: SyntaxState,
    diagnostic: Option<String>,
}

fn compile_python_bytes(bytes: &[u8]) -> SyntaxCompileResult {
    const MAX_CONTROLLED_PYTHON_BYTES: usize = 2 * 1024 * 1024;
    if bytes.len() > MAX_CONTROLLED_PYTHON_BYTES {
        return SyntaxCompileResult {
            state: SyntaxState::Unchecked(SyntaxUncheckedReason::InputTooLarge),
            diagnostic: None,
        };
    }
    let source = match std::str::from_utf8(bytes) {
        Ok(source) => source,
        Err(error) => {
            return SyntaxCompileResult {
                state: SyntaxState::Invalid,
                diagnostic: Some(format!(
                    "source is not UTF-8 at byte {}",
                    error.valid_up_to()
                )),
            };
        }
    };
    compile_python_source(source, || {
        rustpython_compiler::compile(
            source,
            Mode::Exec,
            CANDIDATE_SOURCE_PATH,
            CompileOpts::default(),
        )
        .map(drop)
    })
}

fn compile_python_source(
    source: &str,
    compile: impl FnOnce() -> Result<(), CompileError>,
) -> SyntaxCompileResult {
    let compilation = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // 0.5 implements PEP 695. Retain Ferric's conservative unqualified-
        // surface guard during this compatibility migration; removing it is
        // a separate admission requalification, not an incidental API repair.
        (!source_contains_type_parameters(source)).then(compile)
    }));
    match compilation {
        Ok(Some(Ok(()))) => SyntaxCompileResult {
            state: SyntaxState::Valid,
            diagnostic: None,
        },
        Ok(Some(Err(CompileError::Codegen(error))))
            if matches!(&error.error, CodegenErrorType::NotImplementedYet) =>
        {
            SyntaxCompileResult {
                state: SyntaxState::Unchecked(SyntaxUncheckedReason::CompilerFailure),
                diagnostic: None,
            }
        }
        Ok(Some(Err(error))) => {
            let diagnostic: String = format!("{PYTHON_ADMISSION_COMPILER}: {error}")
                .chars()
                .take(512)
                .collect();
            SyntaxCompileResult {
                state: SyntaxState::Invalid,
                diagnostic: Some(diagnostic),
            }
        }
        Ok(None) | Err(_) => SyntaxCompileResult {
            state: SyntaxState::Unchecked(SyntaxUncheckedReason::CompilerFailure),
            diagnostic: None,
        },
    }
}

#[derive(Default)]
struct TypeParameterScan(bool);

impl<'a> ast::statement_visitor::StatementVisitor<'a> for TypeParameterScan {
    fn visit_stmt(&mut self, statement: &'a ast::Stmt) {
        if self.0 {
            return;
        }
        self.0 = match statement {
            ast::Stmt::FunctionDef(node) => node.type_params.is_some(),
            ast::Stmt::ClassDef(node) => node.type_params.is_some(),
            ast::Stmt::TypeAlias(node) => node.type_params.is_some(),
            _ => false,
        };
        if !self.0 {
            ast::statement_visitor::walk_stmt(self, statement);
        }
    }
}

fn source_contains_type_parameters(source: &str) -> bool {
    use ast::statement_visitor::StatementVisitor;
    parser::parse_module(source).is_ok_and(|module| {
        let mut scan = TypeParameterScan::default();
        scan.visit_body(module.suite());
        scan.0
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::SyntaxUncheckedReason;

    #[test]
    fn clean_python_returns_none() {
        assert!(
            legacy_syntax_warning(Path::new("clean.py"), b"def hello():\n    return 42\n")
                .is_none()
        );
    }

    #[test]
    fn broken_python_returns_in_process_warning() {
        let warning = legacy_syntax_warning(
            Path::new("does-not-need-to-exist.py"),
            b"def fibonacci(6):\n    pass\n",
        )
        .expect("the in-process parser must diagnose invalid source");
        assert!(
            warning.contains("syntax"),
            "expected a syntax error, got: {warning}"
        );
    }

    #[test]
    fn unrecognized_extension_returns_none() {
        assert!(legacy_syntax_warning(Path::new("data.csv"), b"not,code,at,all").is_none());
    }

    #[test]
    fn no_extension_returns_none() {
        assert!(legacy_syntax_warning(Path::new("Makefile"), b"broken syntax {{{{").is_none());
    }

    #[test]
    fn controlled_compiler_bounds_source_size_without_starting_a_process() {
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
    fn controlled_compiler_treats_invalid_utf8_as_invalid_source() {
        let transition =
            candidate_syntax_transition(Path::new("candidate.py"), None, b"value = '\xff'\n");
        assert_eq!(transition.candidate, SyntaxState::Invalid);
        assert!(transition.diagnostic_sha256.is_some());
    }

    #[test]
    fn syntax_regression_matrix_blocks_invalid_from_trusted_or_compiler_failure_baselines() {
        for before in [
            SyntaxState::Absent,
            SyntaxState::Valid,
            SyntaxState::Unchecked(SyntaxUncheckedReason::CompilerFailure),
        ] {
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
            (
                SyntaxState::Valid,
                SyntaxState::Unchecked(SyntaxUncheckedReason::CompilerFailure),
            ),
            (
                SyntaxState::Unchecked(SyntaxUncheckedReason::CompilerFailure),
                SyntaxState::Unchecked(SyntaxUncheckedReason::CompilerFailure),
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
    fn syntax_check_has_no_external_side_effects() {
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
            panic!("in-process compiler must report deterministic invalid syntax")
        };
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        assert!(
            first
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
    }

    #[test]
    fn python_05_admission_matrix() {
        for candidate in [
            b"return\n".as_slice(),
            b"break\n".as_slice(),
            b"continue\n".as_slice(),
        ] {
            for before in [None, Some(b"value = 1\n".as_slice())] {
                let transition =
                    candidate_syntax_transition(Path::new("candidate.py"), before, candidate);
                assert_eq!(transition.candidate, SyntaxState::Invalid);
                assert!(transition.blocks_mutation());
                assert!(transition.diagnostic_sha256.is_some());
            }
        }
    }

    #[test]
    fn contextually_valid_control_flow_compiles() {
        for candidate in [
            b"def function():\n    return 1\n".as_slice(),
            b"while True:\n    break\n".as_slice(),
            b"while True:\n    continue\n".as_slice(),
        ] {
            let transition =
                candidate_syntax_transition(Path::new("candidate.py"), None, candidate);
            assert_eq!(transition.candidate, SyntaxState::Valid);
            assert!(!transition.blocks_mutation());
            assert!(transition.diagnostic_sha256.is_none());
        }
    }

    #[test]
    fn pep_695_alias_without_type_parameters_compiles() {
        let transition =
            candidate_syntax_transition(Path::new("candidate.py"), None, b"type Alias = int\n");
        assert_eq!(transition.candidate, SyntaxState::Valid);
        assert!(!transition.blocks_mutation());
    }

    #[test]
    fn except_star_is_valid() {
        let source = b"try:\n    pass\nexcept* Exception:\n    pass\n";
        let transition = candidate_syntax_transition(Path::new("candidate.py"), None, source);
        assert_eq!(transition.candidate, SyntaxState::Valid);
        assert!(!transition.blocks_mutation());
        assert!(transition.diagnostic_sha256.is_none());
        assert!(legacy_syntax_warning(Path::new("candidate.py"), source).is_none());
    }

    #[test]
    fn compiler_failure_baseline_blocks_a_proven_invalid_candidate() {
        let compiler_unsupported = b"type Alias[T] = list[T]\n";
        let transition = candidate_syntax_transition(
            Path::new("candidate.py"),
            Some(compiler_unsupported),
            b"return\n",
        );
        assert_eq!(
            transition.before,
            SyntaxState::Unchecked(SyntaxUncheckedReason::CompilerFailure)
        );
        assert_eq!(transition.candidate, SyntaxState::Invalid);
        assert!(transition.blocks_mutation());
        assert!(transition.diagnostic_sha256.is_some());
    }

    #[test]
    fn unsupported_codegen_remains_unchecked() {
        let result = compile_python_source("value = 1\n", || {
            Err(CompileError::Codegen(
                rustpython_compiler::codegen::error::CodegenError {
                    location: None,
                    error: CodegenErrorType::NotImplementedYet,
                    source_path: CANDIDATE_SOURCE_PATH.to_owned(),
                },
            ))
        });
        assert_eq!(
            result.state,
            SyntaxState::Unchecked(SyntaxUncheckedReason::CompilerFailure)
        );
        assert!(result.diagnostic.is_none());
        let invalid = compile_python_bytes(b"return\n");
        assert_eq!(invalid.state, SyntaxState::Invalid);
        assert!(
            invalid
                .diagnostic
                .unwrap()
                .starts_with(PYTHON_ADMISSION_COMPILER)
        );
        assert_eq!(PYTHON_ADMISSION_COMPILER, "rustpython-compiler/0.5");
    }

    #[test]
    fn pep_695_type_parameters_are_preflighted_without_invoking_the_compiler() {
        use std::sync::atomic::{AtomicBool, Ordering};

        for source in [
            "type Alias[*Ts] = tuple[*Ts]\n",
            "def identity[T](value: T):\n    return value\n",
            "class Box[T]:\n    pass\n",
            "if True:\n    async def identity[T](value: T):\n        return value\n",
            "try:\n    pass\nexcept Exception:\n    type Alias[T] = list[T]\n",
            "try:\n    pass\nfinally:\n    class Box[T]:\n        pass\n",
            "match value:\n    case 1:\n        type Alias[T] = list[T]\n",
        ] {
            let compiler_called = AtomicBool::new(false);
            let result = compile_python_source(source, || {
                compiler_called.store(true, Ordering::Relaxed);
                Ok(())
            });
            assert_eq!(
                result.state,
                SyntaxState::Unchecked(SyntaxUncheckedReason::CompilerFailure)
            );
            assert!(result.diagnostic.is_none());
            assert!(!compiler_called.load(Ordering::Relaxed));

            let transition =
                candidate_syntax_transition(Path::new("candidate.py"), None, source.as_bytes());
            assert_eq!(transition.candidate, result.state);
            assert!(!transition.blocks_mutation());
            assert!(transition.diagnostic_sha256.is_none());
        }
    }
}

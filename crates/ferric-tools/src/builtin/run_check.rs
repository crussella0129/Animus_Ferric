use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ferric_guard::PermissionLevel;
use serde::Deserialize;
use serde_json::json;
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::spec::{Tool, ToolCtx, ToolSpec};

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const DEFAULT_OUTPUT_LIMIT: usize = 4_000;
const MAX_TIMEOUT_SECS: u64 = 3_600;
const MAX_OUTPUT_LIMIT: usize = 100_000;

fn default_timeout_secs() -> u64 {
    DEFAULT_TIMEOUT_SECS
}

fn default_output_limit() -> usize {
    DEFAULT_OUTPUT_LIMIT
}

/// One operator-authorized verification command. The model sees only `name`;
/// `program` and `args` are fixed before the tool is registered.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedCheck {
    pub name: String,
    pub program: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_timeout_secs")]
    pub timeout_s: u64,
    #[serde(default = "default_output_limit")]
    pub output_limit: usize,
}

#[derive(Debug, Clone)]
struct ResolvedCheck {
    program: PathBuf,
    args: Vec<String>,
    timeout: Duration,
    output_limit: usize,
}

/// Execute one of a closed set of operator-authorized checks.
///
/// This is intentionally not `shell_exec`: the model supplies a name, never a
/// command string, executable, argument, environment variable, or working
/// directory. Bare executable names are resolved through PATH during
/// construction and stored as canonical absolute paths before the model runs.
#[derive(Debug)]
pub struct RunCheck {
    checks: BTreeMap<String, ResolvedCheck>,
}

impl RunCheck {
    pub fn new(checks: Vec<NamedCheck>) -> Result<Self, String> {
        if checks.is_empty() {
            return Err("at least one named check is required".to_string());
        }

        let mut resolved = BTreeMap::new();
        for check in checks {
            validate_name(&check.name)?;
            if check.timeout_s == 0 || check.timeout_s > MAX_TIMEOUT_SECS {
                return Err(format!(
                    "check `{}` timeout_s must be in 1..={MAX_TIMEOUT_SECS}",
                    check.name
                ));
            }
            if check.output_limit == 0 || check.output_limit > MAX_OUTPUT_LIMIT {
                return Err(format!(
                    "check `{}` output_limit must be in 1..={MAX_OUTPUT_LIMIT}",
                    check.name
                ));
            }
            if check.args.len() > 128 || check.args.iter().any(|arg| arg.contains('\0')) {
                return Err(format!(
                    "check `{}` has too many arguments or contains NUL",
                    check.name
                ));
            }
            let name = check.name;
            let value = ResolvedCheck {
                program: resolve_program(&check.program)?,
                args: check.args,
                timeout: Duration::from_secs(check.timeout_s),
                output_limit: check.output_limit,
            };
            if resolved.insert(name.clone(), value).is_some() {
                return Err(format!("duplicate check name `{name}`"));
            }
        }
        Ok(Self { checks: resolved })
    }

    pub fn names(&self) -> Vec<String> {
        self.checks.keys().cloned().collect()
    }

    fn descriptor(&self) -> String {
        self.checks.keys().cloned().collect::<Vec<_>>().join(", ")
    }
}

impl Tool for RunCheck {
    fn spec(&self) -> ToolSpec {
        let names: Vec<String> = self.checks.keys().cloned().collect();
        ToolSpec {
            name: "run_check".to_string(),
            description: format!(
                "Run one operator-authorized verification check in the workspace. \
                 The command and arguments are fixed; choose only its name. \
                 Available checks: {}.",
                self.descriptor()
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "enum": names,
                        "description": "The authorized check to run"
                    }
                },
                "required": ["name"],
                "additionalProperties": false
            }),
            permission: PermissionLevel::Execute,
            ring: 0,
        }
    }

    fn target_paths(&self, _args: &serde_json::Value) -> Vec<String> {
        Vec::new()
    }

    fn target_commands(&self, args: &serde_json::Value) -> Vec<String> {
        let Some(name) = args.get("name").and_then(|value| value.as_str()) else {
            return Vec::new();
        };
        self.checks
            .get(name)
            .map(|check| {
                vec![
                    std::iter::once(check.program.to_string_lossy().into_owned())
                        .chain(check.args.iter().cloned())
                        .collect::<Vec<_>>()
                        .join(" "),
                ]
            })
            .unwrap_or_default()
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let name = args
            .get("name")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "missing required string argument: name".to_string())?;
        let check = self
            .checks
            .get(name)
            .ok_or_else(|| format!("unknown authorized check `{name}`"))?;

        super::blocking::block_on_ambient("run_check", async {
            run_resolved_check(name, check, ctx.workspace.root()).await
        })?
    }
}

async fn run_resolved_check(
    name: &str,
    check: &ResolvedCheck,
    workspace: &Path,
) -> Result<String, String> {
    use std::process::Stdio;

    let mut command = tokio::process::Command::new(&check.program);
    command
        .args(&check.args)
        .current_dir(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command
        .spawn()
        .map_err(|error| format!("check `{name}` failed to spawn: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("check `{name}` stdout was not piped"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("check `{name}` stderr was not piped"))?;

    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();
    let mut stdout_truncated = false;
    let mut stderr_truncated = false;
    let limit = check.output_limit;
    let wait = async {
        let (status, stdout_result, stderr_result) = tokio::join!(
            child.wait(),
            read_bounded(stdout, limit, &mut stdout_buf, &mut stdout_truncated),
            read_bounded(stderr, limit, &mut stderr_buf, &mut stderr_truncated),
        );
        stdout_result.map_err(|error| format!("check `{name}` stdout read failed: {error}"))?;
        stderr_result.map_err(|error| format!("check `{name}` stderr read failed: {error}"))?;
        status.map_err(|error| format!("check `{name}` wait failed: {error}"))
    };

    let status = match tokio::time::timeout(check.timeout, wait).await {
        Ok(result) => result?,
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(format!(
                "check `{name}` timed out after {} second(s)",
                check.timeout.as_secs()
            ));
        }
    };

    let output = render_output(&stdout_buf, stdout_truncated, &stderr_buf, stderr_truncated);
    if status.success() {
        if output.is_empty() {
            Ok(format!("check `{name}` passed"))
        } else {
            Ok(format!("check `{name}` passed\n{output}"))
        }
    } else {
        let status = status
            .code()
            .map_or_else(|| "signal".to_string(), |code| format!("status {code}"));
        if output.is_empty() {
            Err(format!("check `{name}` failed with {status}"))
        } else {
            Err(format!("check `{name}` failed with {status}\n{output}"))
        }
    }
}

async fn read_bounded(
    mut reader: impl AsyncRead + Unpin,
    limit: usize,
    output: &mut Vec<u8>,
    truncated: &mut bool,
) -> std::io::Result<()> {
    let mut chunk = [0_u8; 8_192];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            return Ok(());
        }
        let remaining = limit.saturating_sub(output.len());
        let keep = remaining.min(read);
        output.extend_from_slice(&chunk[..keep]);
        *truncated |= keep < read;
    }
}

fn render_output(
    stdout: &[u8],
    stdout_truncated: bool,
    stderr: &[u8],
    stderr_truncated: bool,
) -> String {
    let mut sections = Vec::new();
    if !stdout.is_empty() || stdout_truncated {
        let mut text = String::from_utf8_lossy(stdout).into_owned();
        if stdout_truncated {
            text.push_str("\n... [stdout truncated]");
        }
        sections.push(format!("stdout:\n{text}"));
    }
    if !stderr.is_empty() || stderr_truncated {
        let mut text = String::from_utf8_lossy(stderr).into_owned();
        if stderr_truncated {
            text.push_str("\n... [stderr truncated]");
        }
        sections.push(format!("stderr:\n{text}"));
    }
    sections.join("\n")
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(format!(
            "invalid check name `{name}` (use 1-64 ASCII letters, digits, '-' or '_')"
        ));
    }
    Ok(())
}

fn resolve_program(program: &Path) -> Result<PathBuf, String> {
    if program.as_os_str().is_empty() {
        return Err("check program cannot be empty".to_string());
    }
    if program.is_absolute() {
        return canonical_executable(program);
    }
    if program.components().count() != 1 {
        return Err(format!(
            "check program `{}` must be an absolute path or a bare executable name",
            program.display()
        ));
    }

    let path = std::env::var_os("PATH").unwrap_or_default();
    for directory in std::env::split_paths(&path) {
        for candidate in executable_candidates(&directory.join(program)) {
            if let Ok(resolved) = canonical_executable(&candidate) {
                return Ok(resolved);
            }
        }
    }
    Err(format!(
        "check program `{}` was not found on PATH",
        program.display()
    ))
}

#[cfg(windows)]
fn executable_candidates(path: &Path) -> Vec<PathBuf> {
    if path.extension().is_some() {
        vec![path.to_path_buf()]
    } else {
        vec![path.with_extension("exe"), path.with_extension("com")]
    }
}

#[cfg(not(windows))]
fn executable_candidates(path: &Path) -> Vec<PathBuf> {
    vec![path.to_path_buf()]
}

fn canonical_executable(path: &Path) -> Result<PathBuf, String> {
    if !path.is_file() {
        return Err(format!("check program `{}` is not a file", path.display()));
    }
    std::fs::canonicalize(path).map_err(|error| {
        format!(
            "cannot canonicalize check program `{}`: {error}",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::ToolCtx;
    use ferric_guard::Workspace;

    fn shell_check(name: &str, script: &str, timeout_s: u64, output_limit: usize) -> NamedCheck {
        #[cfg(windows)]
        let (program, args) = (
            PathBuf::from("powershell.exe"),
            vec![
                "-NoProfile".to_string(),
                "-NonInteractive".to_string(),
                "-Command".to_string(),
                script.to_string(),
            ],
        );
        #[cfg(not(windows))]
        let (program, args) = (
            PathBuf::from("sh"),
            vec!["-c".to_string(), script.to_string()],
        );
        NamedCheck {
            name: name.to_string(),
            program,
            args,
            timeout_s,
            output_limit,
        }
    }

    fn run(tool: &RunCheck, name: &str) -> Result<String, String> {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        tool.run(
            &ToolCtx {
                workspace: &workspace,
            },
            &json!({"name": name}),
        )
    }

    #[test]
    fn duplicate_and_unsafe_names_are_rejected() {
        let duplicate = vec![
            shell_check("test", "exit 0", 5, 100),
            shell_check("test", "exit 0", 5, 100),
        ];
        assert!(RunCheck::new(duplicate).unwrap_err().contains("duplicate"));
        assert!(
            RunCheck::new(vec![shell_check("../test", "exit 0", 5, 100)])
                .unwrap_err()
                .contains("invalid check name")
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn fixed_check_passes_and_unknown_name_fails() {
        #[cfg(windows)]
        let script = "Write-Output pass";
        #[cfg(not(windows))]
        let script = "printf pass";
        let tool = RunCheck::new(vec![shell_check("unit", script, 5, 100)]).unwrap();
        let output = run(&tool, "unit").unwrap();
        assert!(output.contains("check `unit` passed"), "{output}");
        assert!(output.contains("pass"), "{output}");
        assert!(run(&tool, "unknown").unwrap_err().contains("unknown"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn nonzero_and_output_cap_are_reported() {
        #[cfg(windows)]
        let script = "[Console]::Out.Write(('x' * 200)); exit 7";
        #[cfg(not(windows))]
        let script = "head -c 200 /dev/zero | tr '\\0' x; exit 7";
        let tool = RunCheck::new(vec![shell_check("unit", script, 5, 32)]).unwrap();
        let error = run(&tool, "unit").unwrap_err();
        assert!(error.contains("status 7"), "{error}");
        assert!(error.contains("stdout truncated"), "{error}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn timeout_is_a_tool_error() {
        #[cfg(windows)]
        let script = "Start-Sleep -Seconds 2";
        #[cfg(not(windows))]
        let script = "sleep 2";
        let tool = RunCheck::new(vec![shell_check("slow", script, 1, 100)]).unwrap();
        let error = run(&tool, "slow").unwrap_err();
        assert!(error.contains("timed out"), "{error}");
    }
}

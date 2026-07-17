use std::path::Path;
use std::process::Command;

/// Executes a shell command synchronously inside the given workspace root.
/// Under Windows, runs via `cmd.exe /C`. Under Unix, runs via `sh -c`.
pub fn run_hook(cmd_str: &str, workspace_root: &Path) -> Result<String, String> {
    #[cfg(windows)]
    let mut cmd = Command::new("cmd.exe");
    #[cfg(windows)]
    cmd.arg("/C").arg(cmd_str);

    #[cfg(not(windows))]
    let mut cmd = Command::new("sh");
    #[cfg(not(windows))]
    cmd.arg("-c").arg(cmd_str);

    cmd.current_dir(workspace_root);

    let output = cmd.output().map_err(|e| format!("io error: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        let msg = if !stderr.is_empty() { stderr } else { stdout };
        return Err(format!("exit code {}: {}", output.status, msg));
    }

    Ok(stdout)
}

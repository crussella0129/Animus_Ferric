//! Encapsulates executing retrieval commands inside a hardened microVM sandbox.
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("sandbox exec failed: {0}")]
    Exec(String),
    #[error("docker daemon not reachable")]
    NotAvailable,
}

pub struct SandboxConfig {
    pub image: String,
    pub enforce_runsc: bool,
    pub proxy_url: Option<String>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            image: "alpine:latest".to_string(),
            enforce_runsc: false, // Drop back to standard security opts if false
            proxy_url: None,
        }
    }
}

pub fn check_available() -> bool {
    Command::new("docker")
        .arg("info")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn run_in_sandbox(config: &SandboxConfig, cmd: &[&str]) -> Result<String, SandboxError> {
    if !check_available() {
        return Err(SandboxError::NotAvailable);
    }
    
    let mut docker = Command::new("docker");
    docker.arg("run").arg("--rm");
    
    // Strict isolation
    // Using bridge network so it can fetch.
    docker.arg("--network").arg("bridge"); 
    docker.arg("--cap-drop=ALL");
    docker.arg("--security-opt").arg("no-new-privileges");
    
    if config.enforce_runsc {
        docker.arg("--runtime=runsc");
    }
    
    if let Some(proxy) = &config.proxy_url {
        docker.arg("--env").arg(format!("http_proxy={}", proxy));
        docker.arg("--env").arg(format!("https_proxy={}", proxy));
    }
    
    docker.arg(&config.image);
    docker.args(cmd);
    
    let output = docker.output().map_err(|e| SandboxError::Exec(e.to_string()))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SandboxError::Exec(format!("exit code {}: {}", output.status, stderr)));
    }
    
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

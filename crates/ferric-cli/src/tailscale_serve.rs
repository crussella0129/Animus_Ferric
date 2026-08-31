//! Closed, bounded adapter for the one Tailscale Serve coordinate Ferric owns.
//!
//! This module deliberately has no generic command entry point. Read access is
//! limited to `whoami --json` and `serve status --json`; mutation is limited to
//! an exact high-entropy `/_ferric/<token>` path. In particular, there is no
//! route to `reset`, `set-config`, a root-path handler, or an unscoped `off`.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};

pub(crate) const OWNERSHIP_VERSION: u8 = 1;
pub(crate) const HTTPS_PORT: u16 = 443;
const TOKEN_BYTES: usize = 16;
const TOKEN_HEX_LEN: usize = TOKEN_BYTES * 2;
const STATUS_SHA256_HEX_LEN: usize = 64;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const COMMAND_OUTPUT_LIMIT: usize = 256 * 1024;
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Durable, additive ownership evidence carried by a schema-v2 server record.
///
/// `before_status_sha256` is provenance, not teardown authority. Destructive
/// authority always comes from a fresh exact-coordinate comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TailscaleServeOwnership {
    pub version: u8,
    pub token: String,
    pub fqdn: String,
    pub https_port: u16,
    pub mount_path: String,
    pub proxy_target: String,
    pub remote_base_url: String,
    pub before_status_sha256: String,
}

impl TailscaleServeOwnership {
    pub(crate) fn validate(&self) -> Result<(), TailscaleServeError> {
        if self.version != OWNERSHIP_VERSION {
            return Err(TailscaleServeError::InvalidOwnership(format!(
                "unsupported Tailscale Serve ownership version {}",
                self.version
            )));
        }
        validate_token(&self.token)?;
        validate_fqdn(&self.fqdn)?;
        if self.https_port != HTTPS_PORT {
            return Err(TailscaleServeError::InvalidOwnership(format!(
                "Tailscale Serve ownership must use HTTPS port {HTTPS_PORT}"
            )));
        }
        let expected_mount = mount_path_for_token(&self.token)?;
        if self.mount_path != expected_mount {
            return Err(TailscaleServeError::InvalidOwnership(format!(
                "Tailscale Serve mount_path must be {expected_mount}"
            )));
        }
        let port = proxy_target_port(&self.proxy_target)?;
        let expected_target = proxy_target_for_port(port);
        if self.proxy_target != expected_target {
            return Err(TailscaleServeError::InvalidOwnership(format!(
                "Tailscale Serve proxy_target must be {expected_target}"
            )));
        }
        let expected_remote = remote_base_for(&self.fqdn, &self.mount_path);
        if self.remote_base_url != expected_remote {
            return Err(TailscaleServeError::InvalidOwnership(format!(
                "Tailscale Serve remote_base_url must be {expected_remote}"
            )));
        }
        validate_lower_hex(
            "before_status_sha256",
            &self.before_status_sha256,
            STATUS_SHA256_HEX_LEN,
        )?;
        Ok(())
    }

    pub(crate) fn validate_for_port(&self, port: u16) -> Result<(), TailscaleServeError> {
        self.validate()?;
        if port == 0 || self.proxy_target != proxy_target_for_port(port) {
            return Err(TailscaleServeError::InvalidOwnership(format!(
                "Tailscale Serve proxy_target does not match registered loopback port {port}"
            )));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn proxy_port(&self) -> Result<u16, TailscaleServeError> {
        proxy_target_port(&self.proxy_target)
    }
}

/// High-entropy coordinate prepared before any engine or Serve side effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TailscaleServeCoordinate {
    pub token: String,
    pub fqdn: String,
    pub https_port: u16,
    pub mount_path: String,
    pub proxy_target: String,
    pub remote_base_url: String,
}

impl TailscaleServeCoordinate {
    pub(crate) fn into_ownership(
        self,
        before_status_sha256: String,
    ) -> Result<TailscaleServeOwnership, TailscaleServeError> {
        let ownership = TailscaleServeOwnership {
            version: OWNERSHIP_VERSION,
            token: self.token,
            fqdn: self.fqdn,
            https_port: self.https_port,
            mount_path: self.mount_path,
            proxy_target: self.proxy_target,
            remote_base_url: self.remote_base_url,
            before_status_sha256,
        };
        ownership.validate()?;
        Ok(ownership)
    }
}

pub(crate) trait EntropySource {
    fn fill_128(&self, destination: &mut [u8; TOKEN_BYTES]) -> Result<(), TailscaleServeError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct OsEntropy;

impl EntropySource for OsEntropy {
    fn fill_128(&self, destination: &mut [u8; TOKEN_BYTES]) -> Result<(), TailscaleServeError> {
        // Exactly one 16-byte OS-CSPRNG fill: no truncation, expansion, or
        // pseudo-random fallback can silently weaken the ownership coordinate.
        getrandom::fill(destination).map_err(|error| {
            TailscaleServeError::Entropy(format!(
                "could not obtain 128 bits from the operating-system CSPRNG: {error}"
            ))
        })
    }
}

#[cfg(test)]
pub(crate) fn prepare_coordinate_with_entropy<E: EntropySource>(
    port: u16,
    fqdn: &str,
    entropy: &E,
) -> Result<TailscaleServeCoordinate, TailscaleServeError> {
    let token = generate_token_with_entropy(entropy)?;
    coordinate_from_token(port, fqdn, token)
}

/// Draw the ownership coordinate before identity or command probes. This
/// split lets launch preflight prove that entropy failure advances neither an
/// engine counter nor a Tailscale command counter.
pub(crate) fn generate_token() -> Result<String, TailscaleServeError> {
    generate_token_with_entropy(&OsEntropy)
}

pub(crate) fn generate_token_with_entropy<E: EntropySource>(
    entropy: &E,
) -> Result<String, TailscaleServeError> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    entropy.fill_128(&mut bytes)?;
    Ok(hex::encode(bytes))
}

pub(crate) fn coordinate_from_token(
    port: u16,
    fqdn: &str,
    token: String,
) -> Result<TailscaleServeCoordinate, TailscaleServeError> {
    if port == 0 {
        return Err(TailscaleServeError::InvalidOwnership(
            "Tailscale Serve requires a nonzero loopback target port".to_string(),
        ));
    }
    validate_fqdn(fqdn)?;
    let mount_path = mount_path_for_token(&token)?;
    let proxy_target = proxy_target_for_port(port);
    let remote_base_url = remote_base_for(fqdn, &mount_path);
    Ok(TailscaleServeCoordinate {
        token,
        fqdn: fqdn.to_string(),
        https_port: HTTPS_PORT,
        mount_path,
        proxy_target,
        remote_base_url,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ServePathState {
    Absent,
    Proxy { target: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServeStatusObservation {
    pub fqdn: String,
    pub https_port: u16,
    pub mount_path: String,
    pub status_sha256: String,
    pub path_state: ServePathState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OwnedServeState {
    Absent,
    Exact,
    Replaced { observed_target: String },
}

impl ServeStatusObservation {
    pub(crate) fn owned_state(
        &self,
        ownership: &TailscaleServeOwnership,
    ) -> Result<OwnedServeState, TailscaleServeError> {
        ownership.validate()?;
        if self.fqdn != ownership.fqdn
            || self.https_port != ownership.https_port
            || self.mount_path != ownership.mount_path
        {
            return Err(TailscaleServeError::InvalidStatus(
                "Serve observation does not describe the ownership coordinate".to_string(),
            ));
        }
        Ok(match &self.path_state {
            ServePathState::Absent => OwnedServeState::Absent,
            ServePathState::Proxy { target } if target == &ownership.proxy_target => {
                OwnedServeState::Exact
            }
            ServePathState::Proxy { target } => OwnedServeState::Replaced {
                observed_target: target.clone(),
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TailscaleServeError {
    Entropy(String),
    CommandSpawn(String),
    CommandTimeout,
    CommandOutputLimit,
    CommandExit { operation: &'static str },
    InvalidIdentity(String),
    InvalidStatus(String),
    InvalidOwnership(String),
}

impl fmt::Display for TailscaleServeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Entropy(detail) => write!(formatter, "{detail}"),
            Self::CommandSpawn(detail) => write!(formatter, "{detail}"),
            Self::CommandTimeout => write!(
                formatter,
                "Tailscale CLI exceeded its bounded execution time"
            ),
            Self::CommandOutputLimit => write!(
                formatter,
                "Tailscale CLI exceeded its bounded output allowance"
            ),
            Self::CommandExit { operation } => {
                write!(formatter, "Tailscale CLI {operation} failed")
            }
            Self::InvalidIdentity(detail) => {
                write!(formatter, "invalid Tailscale identity: {detail}")
            }
            Self::InvalidStatus(detail) => {
                write!(formatter, "invalid Tailscale Serve status: {detail}")
            }
            Self::InvalidOwnership(detail) => {
                write!(formatter, "invalid Tailscale Serve ownership: {detail}")
            }
        }
    }
}

impl std::error::Error for TailscaleServeError {}

#[derive(Debug)]
pub(crate) struct CommandOutcome {
    stdout: Vec<u8>,
}

pub(crate) trait CommandRunner {
    fn run(
        &self,
        program: &Path,
        args: &[String],
        operation: &'static str,
    ) -> Result<CommandOutcome, TailscaleServeError>;
}

#[derive(Debug, Clone, Copy)]
struct CommandLimits {
    timeout: Duration,
    output_limit: usize,
    poll_interval: Duration,
}

impl Default for CommandLimits {
    fn default() -> Self {
        Self {
            timeout: COMMAND_TIMEOUT,
            output_limit: COMMAND_OUTPUT_LIMIT,
            poll_interval: COMMAND_POLL_INTERVAL,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NativeCommandRunner {
    limits: CommandLimits,
}

impl CommandRunner for NativeCommandRunner {
    fn run(
        &self,
        program: &Path,
        args: &[String],
        operation: &'static str,
    ) -> Result<CommandOutcome, TailscaleServeError> {
        run_native_command(program, args, operation, self.limits)
    }
}

/// The production adapter is constructible only with a program path; its argv
/// is closed by the four methods below.
#[derive(Debug)]
pub(crate) struct TailscaleServeAdapter<R = NativeCommandRunner> {
    program: PathBuf,
    runner: R,
}

impl TailscaleServeAdapter<NativeCommandRunner> {
    pub(crate) fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            runner: NativeCommandRunner::default(),
        }
    }

    /// Use the platform-resolved `tailscale` executable with the closed argv
    /// surface and production bounds.
    pub(crate) fn native() -> Self {
        Self::new("tailscale")
    }

    #[cfg(test)]
    fn with_native_limits(
        program: impl Into<PathBuf>,
        timeout: Duration,
        output_limit: usize,
    ) -> Self {
        Self {
            program: program.into(),
            runner: NativeCommandRunner {
                limits: CommandLimits {
                    timeout,
                    output_limit,
                    poll_interval: Duration::from_millis(1),
                },
            },
        }
    }
}

impl<R> TailscaleServeAdapter<R> {
    #[cfg(test)]
    fn with_runner(program: impl Into<PathBuf>, runner: R) -> Self {
        Self {
            program: program.into(),
            runner,
        }
    }
}

pub(crate) trait TailscaleServeEffects {
    fn self_fqdn(&self) -> Result<String, TailscaleServeError>;
    fn probe_status(&self, fqdn: &str) -> Result<String, TailscaleServeError>;
    fn observe_coordinate(
        &self,
        fqdn: &str,
        mount_path: &str,
    ) -> Result<ServeStatusObservation, TailscaleServeError>;
    fn apply(&self, ownership: &TailscaleServeOwnership) -> Result<(), TailscaleServeError>;
    fn off(&self, ownership: &TailscaleServeOwnership) -> Result<(), TailscaleServeError>;
}

impl<R: CommandRunner> TailscaleServeEffects for TailscaleServeAdapter<R> {
    fn self_fqdn(&self) -> Result<String, TailscaleServeError> {
        let args = strings(&["whoami", "--json"]);
        let output = self.runner.run(&self.program, &args, "identity probe")?;
        parse_self_fqdn(&output.stdout)
    }

    fn probe_status(&self, fqdn: &str) -> Result<String, TailscaleServeError> {
        validate_fqdn(fqdn)?;
        let args = strings(&["serve", "status", "--json"]);
        let output = self
            .runner
            .run(&self.program, &args, "Serve status probe")?;
        validate_status_snapshot(&output.stdout, fqdn)
    }

    fn observe_coordinate(
        &self,
        fqdn: &str,
        mount_path: &str,
    ) -> Result<ServeStatusObservation, TailscaleServeError> {
        validate_fqdn(fqdn)?;
        validate_mount_path(mount_path)?;
        let args = strings(&["serve", "status", "--json"]);
        let output = self
            .runner
            .run(&self.program, &args, "Serve status probe")?;
        project_status(&output.stdout, fqdn, mount_path)
    }

    fn apply(&self, ownership: &TailscaleServeOwnership) -> Result<(), TailscaleServeError> {
        ownership.validate()?;
        let args = vec![
            "serve".to_string(),
            "--bg".to_string(),
            format!("--https={HTTPS_PORT}"),
            format!("--set-path={}", ownership.mount_path),
            "--yes".to_string(),
            ownership.proxy_target.clone(),
        ];
        self.runner
            .run(&self.program, &args, "endpoint apply")
            .map(|_| ())
    }

    fn off(&self, ownership: &TailscaleServeOwnership) -> Result<(), TailscaleServeError> {
        ownership.validate()?;
        let args = vec![
            "serve".to_string(),
            "--bg".to_string(),
            format!("--https={HTTPS_PORT}"),
            format!("--set-path={}", ownership.mount_path),
            "--yes".to_string(),
            "off".to_string(),
        ];
        self.runner
            .run(&self.program, &args, "endpoint removal")
            .map(|_| ())
    }
}

pub(crate) fn parse_self_fqdn(raw: &[u8]) -> Result<String, TailscaleServeError> {
    let value = parse_duplicate_safe_json(raw).map_err(|detail| {
        TailscaleServeError::InvalidIdentity(format!(
            "`tailscale whoami --json` is malformed ({detail}); Tailscale 1.102.1 or newer is required"
        ))
    })?;
    let root = value.as_object().ok_or_else(|| {
        TailscaleServeError::InvalidIdentity(
            "expected an object from `tailscale whoami --json`; Tailscale 1.102.1 or newer is required"
                .to_string(),
        )
    })?;
    let node = root.get("Node").and_then(Value::as_object).ok_or_else(|| {
        TailscaleServeError::InvalidIdentity(
            "missing Node object; upgrade to Tailscale 1.102.1 or newer".to_string(),
        )
    })?;
    let raw_name = node.get("Name").and_then(Value::as_str).ok_or_else(|| {
        TailscaleServeError::InvalidIdentity(
            "missing canonical Node.Name; upgrade to Tailscale 1.102.1 or newer".to_string(),
        )
    })?;
    let name = raw_name.strip_suffix('.').ok_or_else(|| {
        TailscaleServeError::InvalidIdentity(
            "canonical Node.Name must end in exactly one trailing dot".to_string(),
        )
    })?;
    if name.ends_with('.') {
        return Err(TailscaleServeError::InvalidIdentity(
            "Node.Name has more than one trailing dot".to_string(),
        ));
    }
    validate_fqdn(name)?;
    Ok(name.to_string())
}

pub(crate) fn project_status(
    raw: &[u8],
    fqdn: &str,
    mount_path: &str,
) -> Result<ServeStatusObservation, TailscaleServeError> {
    validate_fqdn(fqdn)?;
    validate_mount_path(mount_path)?;
    let value = parse_duplicate_safe_json(raw).map_err(TailscaleServeError::InvalidStatus)?;
    let root = value.as_object().ok_or_else(|| {
        TailscaleServeError::InvalidStatus("status root must be an object".to_string())
    })?;
    let expected_host = format!("{fqdn}:{HTTPS_PORT}");
    let https_mode_present = ensure_compatible_https_port(root, &expected_host)?;
    let mut matches = Vec::new();
    let mut expected_web_host_present = false;
    if let Some(web_value) = root.get("Web") {
        let web = web_value.as_object().ok_or_else(|| {
            TailscaleServeError::InvalidStatus("Web must be an object".to_string())
        })?;
        for (host, web_server_value) in web {
            expected_web_host_present |= host == &expected_host;
            let Some(web_server) = web_server_value.as_object() else {
                if host == &expected_host {
                    return Err(TailscaleServeError::InvalidStatus(format!(
                        "owned Web entry {expected_host} must be an object"
                    )));
                }
                continue;
            };
            let Some(handlers_value) = web_server.get("Handlers") else {
                continue;
            };
            let Some(handlers) = handlers_value.as_object() else {
                if host == &expected_host {
                    return Err(TailscaleServeError::InvalidStatus(format!(
                        "Handlers for {expected_host} must be an object"
                    )));
                }
                continue;
            };
            if let Some(handler) = handlers.get(mount_path) {
                matches.push((host.as_str(), handler));
            }
        }
    }
    if expected_web_host_present && !https_mode_present {
        return Err(TailscaleServeError::InvalidStatus(format!(
            "Web host {expected_host} exists without compatible TCP HTTPS mode"
        )));
    }

    if matches.len() > 1 {
        return Err(TailscaleServeError::InvalidStatus(format!(
            "token path {mount_path} appears at more than one Web coordinate"
        )));
    }
    let path_state = match matches.pop() {
        None => ServePathState::Absent,
        Some((host, _)) if host != expected_host => {
            return Err(TailscaleServeError::InvalidStatus(format!(
                "token path {mount_path} appears at unexpected Web host {host}"
            )));
        }
        Some((_, handler)) => ServePathState::Proxy {
            target: exact_proxy_target(handler, mount_path)?.to_string(),
        },
    };

    Ok(ServeStatusObservation {
        fqdn: fqdn.to_string(),
        https_port: HTTPS_PORT,
        mount_path: mount_path.to_string(),
        status_sha256: canonical_status_sha256(&value),
        path_state,
    })
}

pub(crate) fn validate_status_snapshot(
    raw: &[u8],
    fqdn: &str,
) -> Result<String, TailscaleServeError> {
    validate_fqdn(fqdn)?;
    let value = parse_duplicate_safe_json(raw).map_err(TailscaleServeError::InvalidStatus)?;
    let root = value.as_object().ok_or_else(|| {
        TailscaleServeError::InvalidStatus("status root must be an object".to_string())
    })?;
    let expected_host = format!("{fqdn}:{HTTPS_PORT}");
    let https_mode_present = ensure_compatible_https_port(root, &expected_host)?;
    let mut expected_web_host_present = false;
    if let Some(web_value) = root.get("Web") {
        let web = web_value.as_object().ok_or_else(|| {
            TailscaleServeError::InvalidStatus("Web must be an object".to_string())
        })?;
        if let Some(web_server_value) = web.get(&expected_host) {
            expected_web_host_present = true;
            let web_server = web_server_value.as_object().ok_or_else(|| {
                TailscaleServeError::InvalidStatus(format!(
                    "owned Web entry {expected_host} must be an object"
                ))
            })?;
            if let Some(handlers) = web_server.get("Handlers")
                && !handlers.is_object()
            {
                return Err(TailscaleServeError::InvalidStatus(format!(
                    "Handlers for {expected_host} must be an object"
                )));
            }
        }
    }
    if expected_web_host_present && !https_mode_present {
        return Err(TailscaleServeError::InvalidStatus(format!(
            "Web host {expected_host} exists without compatible TCP HTTPS mode"
        )));
    }
    Ok(canonical_status_sha256(&value))
}

fn ensure_compatible_https_port(
    root: &Map<String, Value>,
    expected_host: &str,
) -> Result<bool, TailscaleServeError> {
    if let Some(funnel_value) = root.get("AllowFunnel") {
        let funnel = funnel_value.as_object().ok_or_else(|| {
            TailscaleServeError::InvalidStatus(
                "AllowFunnel must be an object when present".to_string(),
            )
        })?;
        if let Some(value) = funnel.get(expected_host) {
            match value.as_bool() {
                Some(false) => {}
                Some(true) => {
                    return Err(TailscaleServeError::InvalidStatus(format!(
                        "Web host {expected_host} is configured for Funnel, not private Serve"
                    )));
                }
                None => {
                    return Err(TailscaleServeError::InvalidStatus(format!(
                        "AllowFunnel state for {expected_host} must be boolean"
                    )));
                }
            }
        }
    }
    let Some(tcp_value) = root.get("TCP") else {
        return Ok(false);
    };
    let tcp = tcp_value.as_object().ok_or_else(|| {
        TailscaleServeError::InvalidStatus("TCP must be an object when present".to_string())
    })?;
    let Some(port_value) = tcp.get(&HTTPS_PORT.to_string()) else {
        return Ok(false);
    };
    let port = port_value.as_object().ok_or_else(|| {
        TailscaleServeError::InvalidStatus(format!("TCP port {HTTPS_PORT} must be an object"))
    })?;
    if port.get("HTTPS").and_then(Value::as_bool) != Some(true) {
        return Err(TailscaleServeError::InvalidStatus(format!(
            "TCP port {HTTPS_PORT} is not in compatible HTTPS mode"
        )));
    }
    let incompatible_field = ["HTTP", "TCPForward", "TerminateTLS"]
        .into_iter()
        .find(|field| {
            port.get(*field).is_some_and(|value| match value {
                Value::Null => false,
                Value::Bool(false) => false,
                Value::String(value) if value.is_empty() => false,
                _ => true,
            })
        });
    if let Some(field) = incompatible_field {
        return Err(TailscaleServeError::InvalidStatus(format!(
            "TCP port {HTTPS_PORT} has incompatible {field} mode"
        )));
    }
    Ok(true)
}

fn exact_proxy_target<'a>(
    handler: &'a Value,
    mount_path: &str,
) -> Result<&'a str, TailscaleServeError> {
    let object = handler.as_object().ok_or_else(|| {
        TailscaleServeError::InvalidStatus(format!("handler at {mount_path} must be an object"))
    })?;
    let proxy = object.get("Proxy").and_then(Value::as_str).ok_or_else(|| {
        TailscaleServeError::InvalidStatus(format!(
            "handler at {mount_path} is not an exact proxy handler"
        ))
    })?;
    if object.get("Text").is_some_and(|value| !value.is_null())
        || object.get("Path").is_some_and(|value| !value.is_null())
    {
        return Err(TailscaleServeError::InvalidStatus(format!(
            "handler at {mount_path} combines proxy and non-proxy behavior"
        )));
    }
    Ok(proxy)
}

fn run_native_command(
    program: &Path,
    args: &[String],
    operation: &'static str,
    limits: CommandLimits,
) -> Result<CommandOutcome, TailscaleServeError> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            TailscaleServeError::CommandSpawn(format!(
                "could not launch the Tailscale CLI for {operation}: {error}"
            ))
        })?;

    let stdout = child.stdout.take().ok_or_else(|| {
        TailscaleServeError::CommandSpawn("could not retain bounded Tailscale stdout".to_string())
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        TailscaleServeError::CommandSpawn("could not retain bounded Tailscale stderr".to_string())
    })?;
    let total = Arc::new(AtomicUsize::new(0));
    let exceeded = Arc::new(AtomicBool::new(false));
    let stdout_reader = spawn_bounded_reader(
        stdout,
        Arc::clone(&total),
        Arc::clone(&exceeded),
        limits.output_limit,
    );
    let stderr_reader = spawn_bounded_reader(
        stderr,
        Arc::clone(&total),
        Arc::clone(&exceeded),
        limits.output_limit,
    );

    let deadline = Instant::now() + limits.timeout;
    let terminal: Result<ExitStatus, TailscaleServeError> = loop {
        if exceeded.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            break Err(TailscaleServeError::CommandOutputLimit);
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(TailscaleServeError::CommandTimeout);
            }
            Ok(None) => thread::sleep(limits.poll_interval),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(TailscaleServeError::CommandSpawn(format!(
                    "could not wait for bounded Tailscale {operation}: {error}"
                )));
            }
        }
    };

    let stdout = join_bounded_reader(stdout_reader, "stdout")?;
    // Drain and join stderr, but never return it: identity and tailnet data are
    // not safe to copy into persistent Ferric diagnostics.
    let _stderr = join_bounded_reader(stderr_reader, "stderr")?;
    if exceeded.load(Ordering::Acquire) || total.load(Ordering::Acquire) > limits.output_limit {
        return Err(TailscaleServeError::CommandOutputLimit);
    }
    let status = terminal?;
    if !status.success() {
        return Err(TailscaleServeError::CommandExit { operation });
    }
    Ok(CommandOutcome { stdout })
}

fn spawn_bounded_reader<R: Read + Send + 'static>(
    mut reader: R,
    total: Arc<AtomicUsize>,
    exceeded: Arc<AtomicBool>,
    output_limit: usize,
) -> thread::JoinHandle<std::io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut retained = Vec::new();
        let mut chunk = [0_u8; 8 * 1024];
        loop {
            let count = reader.read(&mut chunk)?;
            if count == 0 {
                break;
            }
            let previous = total.fetch_add(count, Ordering::AcqRel);
            if previous.saturating_add(count) > output_limit {
                exceeded.store(true, Ordering::Release);
            }
            if previous < output_limit {
                let keep = count.min(output_limit - previous);
                retained.extend_from_slice(&chunk[..keep]);
            }
        }
        Ok(retained)
    })
}

fn join_bounded_reader(
    handle: thread::JoinHandle<std::io::Result<Vec<u8>>>,
    channel: &'static str,
) -> Result<Vec<u8>, TailscaleServeError> {
    handle
        .join()
        .map_err(|_| {
            TailscaleServeError::CommandSpawn(format!(
                "bounded Tailscale {channel} reader panicked"
            ))
        })?
        .map_err(|error| {
            TailscaleServeError::CommandSpawn(format!(
                "could not read bounded Tailscale {channel}: {error}"
            ))
        })
}

fn proxy_target_for_port(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

fn proxy_target_port(target: &str) -> Result<u16, TailscaleServeError> {
    let raw_port = target.strip_prefix("http://127.0.0.1:").ok_or_else(|| {
        TailscaleServeError::InvalidOwnership(
            "proxy_target must use exact loopback origin http://127.0.0.1:<port>".to_string(),
        )
    })?;
    let port = raw_port.parse::<u16>().map_err(|_| {
        TailscaleServeError::InvalidOwnership(
            "proxy_target must end in a canonical nonzero u16 port".to_string(),
        )
    })?;
    if port == 0 || raw_port != port.to_string() {
        return Err(TailscaleServeError::InvalidOwnership(
            "proxy_target must end in a canonical nonzero u16 port".to_string(),
        ));
    }
    Ok(port)
}

fn remote_base_for(fqdn: &str, mount_path: &str) -> String {
    format!("https://{fqdn}{mount_path}/v1")
}

fn mount_path_for_token(token: &str) -> Result<String, TailscaleServeError> {
    validate_token(token)?;
    Ok(format!("/_ferric/{token}"))
}

fn validate_mount_path(path: &str) -> Result<(), TailscaleServeError> {
    let token = path.strip_prefix("/_ferric/").ok_or_else(|| {
        TailscaleServeError::InvalidOwnership(
            "mount path must use /_ferric/<32-lowercase-hex>".to_string(),
        )
    })?;
    if token.contains('/') {
        return Err(TailscaleServeError::InvalidOwnership(
            "mount path must name exactly one token segment".to_string(),
        ));
    }
    validate_token(token)
}

fn validate_token(token: &str) -> Result<(), TailscaleServeError> {
    validate_lower_hex("token", token, TOKEN_HEX_LEN)
}

fn validate_lower_hex(
    label: &str,
    value: &str,
    expected_len: usize,
) -> Result<(), TailscaleServeError> {
    if value.len() != expected_len
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(TailscaleServeError::InvalidOwnership(format!(
            "{label} must be exactly {expected_len} lowercase hexadecimal characters"
        )));
    }
    Ok(())
}

fn validate_fqdn(fqdn: &str) -> Result<(), TailscaleServeError> {
    if fqdn.is_empty()
        || fqdn.len() > 253
        || fqdn.ends_with('.')
        || fqdn.bytes().any(|byte| !byte.is_ascii())
        || fqdn.bytes().any(|byte| byte.is_ascii_uppercase())
    {
        return Err(TailscaleServeError::InvalidIdentity(
            "Node.Name must be a canonical lowercase ASCII FQDN without a trailing dot".to_string(),
        ));
    }
    let labels = fqdn.split('.').collect::<Vec<_>>();
    if labels.len() < 2
        || labels.iter().any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
    {
        return Err(TailscaleServeError::InvalidIdentity(
            "Node.Name must be a canonical lowercase DNS name".to_string(),
        ));
    }
    Ok(())
}

fn canonical_status_sha256(value: &Value) -> String {
    let canonical = canonicalize_value(value);
    let bytes = serde_json::to_vec(&canonical).expect("serde_json::Value always serializes");
    hex::encode(Sha256::digest(bytes))
}

fn canonicalize_value(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonicalize_value).collect()),
        Value::Object(object) => {
            let sorted = object
                .iter()
                .map(|(key, value)| (key.clone(), canonicalize_value(value)))
                .collect::<BTreeMap<_, _>>();
            let mut canonical = Map::new();
            for (key, value) in sorted {
                canonical.insert(key, value);
            }
            Value::Object(canonical)
        }
        scalar => scalar.clone(),
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

/// Deserialize JSON while rejecting duplicate keys at every depth. Ordinary
/// `serde_json::Value` parsing retains only the last duplicate, which would
/// erase precisely the ambiguity this adapter must fail closed on.
fn parse_duplicate_safe_json(raw: &[u8]) -> Result<Value, String> {
    serde_json::from_slice::<DuplicateSafeValue>(raw)
        .map(|value| value.0)
        .map_err(|error| error.to_string())
}

struct DuplicateSafeValue(Value);

impl<'de> Deserialize<'de> for DuplicateSafeValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(DuplicateSafeVisitor)
    }
}

struct DuplicateSafeVisitor;

impl<'de> Visitor<'de> for DuplicateSafeVisitor {
    type Value = DuplicateSafeValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON without duplicate object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(DuplicateSafeValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(DuplicateSafeValue(Value::Number(Number::from(value))))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(DuplicateSafeValue(Value::Number(Number::from(value))))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
        Number::from_f64(value)
            .map(Value::Number)
            .map(DuplicateSafeValue)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_string(value.to_string())
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(DuplicateSafeValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(DuplicateSafeValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(DuplicateSafeValue(Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        DuplicateSafeValue::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<DuplicateSafeValue>()? {
            values.push(value.0);
        }
        Ok(DuplicateSafeValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut object = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate object key {key:?}")));
            }
            let value = map.next_value::<DuplicateSafeValue>()?;
            object.insert(key, value.0);
        }
        Ok(DuplicateSafeValue(Value::Object(object)))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use super::*;

    const FQDN: &str = "example-host.tailnet-example.ts.net";
    const TOKEN: &str = "00112233445566778899aabbccddeeff";
    const MOUNT: &str = "/_ferric/00112233445566778899aabbccddeeff";

    struct FixedEntropy(Result<[u8; TOKEN_BYTES], &'static str>);

    impl EntropySource for FixedEntropy {
        fn fill_128(&self, destination: &mut [u8; TOKEN_BYTES]) -> Result<(), TailscaleServeError> {
            match self.0 {
                Ok(bytes) => {
                    *destination = bytes;
                    Ok(())
                }
                Err(detail) => Err(TailscaleServeError::Entropy(detail.to_string())),
            }
        }
    }

    #[derive(Default)]
    struct FakeRunner {
        replies: Mutex<VecDeque<Result<Vec<u8>, TailscaleServeError>>>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl FakeRunner {
        fn with_replies(replies: Vec<Result<&'static [u8], TailscaleServeError>>) -> Self {
            Self {
                replies: Mutex::new(
                    replies
                        .into_iter()
                        .map(|reply| reply.map(<[u8]>::to_vec))
                        .collect(),
                ),
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(
            &self,
            _program: &Path,
            args: &[String],
            _operation: &'static str,
        ) -> Result<CommandOutcome, TailscaleServeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(Vec::new()))
                .map(|stdout| CommandOutcome { stdout })
        }
    }

    fn ownership() -> TailscaleServeOwnership {
        TailscaleServeOwnership {
            version: OWNERSHIP_VERSION,
            token: TOKEN.to_string(),
            fqdn: FQDN.to_string(),
            https_port: HTTPS_PORT,
            mount_path: MOUNT.to_string(),
            proxy_target: "http://127.0.0.1:8080".to_string(),
            remote_base_url: format!("https://{FQDN}{MOUNT}/v1"),
            before_status_sha256: "a".repeat(64),
        }
    }

    #[test]
    fn serve_status_projects_only_exact_token_path() {
        let exact = format!(
            r#"{{"Web":{{"{FQDN}:443":{{"Handlers":{{"/unrelated":{{"Text":"kept"}},"{MOUNT}/longer":{{"Proxy":"http://127.0.0.1:1"}},"{MOUNT}":{{"Proxy":"http://127.0.0.1:8080","FutureField":true}}}}}}}},"Future":{{"kept":true}},"TCP":{{"443":{{"HTTPS":true}}}}}}"#
        );
        let projected = project_status(exact.as_bytes(), FQDN, MOUNT).unwrap();
        assert_eq!(
            projected.path_state,
            ServePathState::Proxy {
                target: "http://127.0.0.1:8080".to_string()
            }
        );
        assert_eq!(
            projected.owned_state(&ownership()).unwrap(),
            OwnedServeState::Exact
        );
        assert_eq!(projected.status_sha256.len(), 64);

        let reordered = format!(
            r#"{{"TCP":{{"443":{{"HTTPS":true}}}},"Future":{{"kept":true}},"Web":{{"{FQDN}:443":{{"Handlers":{{"{MOUNT}":{{"FutureField":true,"Proxy":"http://127.0.0.1:8080"}},"{MOUNT}/longer":{{"Proxy":"http://127.0.0.1:1"}},"/unrelated":{{"Text":"kept"}}}}}}}}}}"#
        );
        let reordered = project_status(reordered.as_bytes(), FQDN, MOUNT).unwrap();
        assert_eq!(projected.status_sha256, reordered.status_sha256);

        let absent = project_status(br#"{"Web":{},"TCP":{}}"#, FQDN, MOUNT).unwrap();
        assert_eq!(absent.path_state, ServePathState::Absent);
    }

    #[test]
    fn serve_status_rejects_non_authorizing_shapes() {
        let malformed = [
            "[]".to_string(),
            r#"{"Web":7}"#.to_string(),
            format!(r#"{{"Web":{{"{FQDN}:443":{{"Handlers":7}}}}}}"#),
            format!(
                r#"{{"Web":{{"{FQDN}:443":{{"Handlers":{{"{MOUNT}":{{"Text":"not proxy"}}}}}}}}}}"#
            ),
            format!(r#"{{"Web":{{"{FQDN}:443":{{"Handlers":{{"{MOUNT}":{{"Proxy":7}}}}}}}}}}"#),
            r#"{"TCP":{"443":{"TCPForward":"127.0.0.1:9"}},"Web":{}}"#.to_string(),
            format!(
                r#"{{"TCP":{{"443":{{"HTTPS":true}}}},"AllowFunnel":{{"{FQDN}:443":true}},"Web":{{}}}}"#
            ),
            format!(r#"{{"Web":{{"{FQDN}:443":{{"Handlers":{{}}}}}}}}"#),
            format!(
                r#"{{"Web":{{"{FQDN}:443":{{"Handlers":{{"{MOUNT}":{{"Proxy":"http://127.0.0.1:8080"}},"{MOUNT}":{{"Proxy":"http://127.0.0.1:8081"}}}}}}}}}}"#
            ),
            format!(
                r#"{{"Web":{{"{FQDN}:443":{{"Handlers":{{"{MOUNT}":{{"Proxy":"http://127.0.0.1:8080"}}}}}},"other.tailnet-example.ts.net:443":{{"Handlers":{{"{MOUNT}":{{"Proxy":"http://127.0.0.1:8080"}}}}}}}}}}"#
            ),
        ];
        for raw in malformed {
            assert!(
                project_status(raw.as_bytes(), FQDN, MOUNT).is_err(),
                "accepted {raw}"
            );
        }

        let runner = FakeRunner::with_replies(vec![Err(TailscaleServeError::CommandTimeout)]);
        let adapter = TailscaleServeAdapter::with_runner("tailscale", runner);
        assert!(adapter.observe_coordinate(FQDN, MOUNT).is_err());
        assert_eq!(adapter.runner.calls.lock().unwrap().len(), 1);

        let current_exe = std::env::current_exe().unwrap();
        let timeout_adapter = TailscaleServeAdapter::with_native_limits(
            &current_exe,
            Duration::from_millis(20),
            COMMAND_OUTPUT_LIMIT,
        );
        let timeout = timeout_adapter.runner.run(
            &current_exe,
            &strings(&[
                "--ignored",
                "--exact",
                "tailscale_serve::tests::native_runner_sleep_helper",
            ]),
            "test timeout",
        );
        assert!(matches!(timeout, Err(TailscaleServeError::CommandTimeout)));

        let output_adapter =
            TailscaleServeAdapter::with_native_limits(&current_exe, Duration::from_secs(5), 1);
        let oversized =
            output_adapter
                .runner
                .run(&current_exe, &strings(&["--list"]), "test output bound");
        assert!(matches!(
            oversized,
            Err(TailscaleServeError::CommandOutputLimit)
        ));
    }

    #[test]
    fn serve_commands_are_closed_and_endpoint_scoped() {
        let runner = FakeRunner::with_replies(vec![Ok(b""), Ok(b"")]);
        let adapter = TailscaleServeAdapter::with_runner("tailscale", runner);
        adapter.apply(&ownership()).unwrap();
        adapter.off(&ownership()).unwrap();
        assert_eq!(
            *adapter.runner.calls.lock().unwrap(),
            vec![
                vec![
                    "serve",
                    "--bg",
                    "--https=443",
                    "--set-path=/_ferric/00112233445566778899aabbccddeeff",
                    "--yes",
                    "http://127.0.0.1:8080",
                ],
                vec![
                    "serve",
                    "--bg",
                    "--https=443",
                    "--set-path=/_ferric/00112233445566778899aabbccddeeff",
                    "--yes",
                    "off",
                ],
            ]
            .into_iter()
            .map(|args| args.into_iter().map(str::to_string).collect())
            .collect::<Vec<Vec<String>>>()
        );
    }

    #[test]
    fn serve_read_only_probes_are_closed() {
        let runner = FakeRunner::with_replies(vec![
            Ok(br#"{"Node":{"Name":"example-host.tailnet-example.ts.net."}}"#),
            Ok(br#"{"Web":{},"TCP":{}}"#),
        ]);
        let adapter = TailscaleServeAdapter::with_runner("tailscale", runner);
        let fqdn = adapter.self_fqdn().unwrap();
        let digest = adapter.probe_status(&fqdn).unwrap();
        assert_eq!(fqdn, FQDN);
        assert_eq!(digest.len(), STATUS_SHA256_HEX_LEN);
        assert_eq!(
            *adapter.runner.calls.lock().unwrap(),
            vec![
                vec!["whoami".to_string(), "--json".to_string()],
                vec![
                    "serve".to_string(),
                    "status".to_string(),
                    "--json".to_string(),
                ],
            ]
        );

        assert!(validate_status_snapshot(b"[]", FQDN).is_err());
        assert!(validate_status_snapshot(br#"{"Web":7}"#, FQDN).is_err());
        assert!(
            validate_status_snapshot(
                br#"{"Web":{"example-host.tailnet-example.ts.net:443":{"Handlers":7}}}"#,
                FQDN,
            )
            .is_err()
        );
    }

    #[test]
    fn ownership_token_and_remote_base_are_valid() {
        let bytes = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ];
        let coordinate =
            prepare_coordinate_with_entropy(8080, FQDN, &FixedEntropy(Ok(bytes))).unwrap();
        assert_eq!(coordinate.token, TOKEN);
        assert_eq!(coordinate.mount_path, MOUNT);
        assert_eq!(coordinate.proxy_target, "http://127.0.0.1:8080");
        assert_eq!(
            coordinate.remote_base_url,
            format!("https://{FQDN}{MOUNT}/v1")
        );
        let ownership = coordinate.into_ownership("f".repeat(64)).unwrap();
        ownership.validate_for_port(8080).unwrap();
        assert_eq!(ownership.proxy_port().unwrap(), 8080);

        assert_eq!(
            parse_self_fqdn(format!(r#"{{"Node":{{"Name":"{FQDN}."}}}}"#).as_bytes()).unwrap(),
            FQDN
        );
        assert!(parse_self_fqdn(format!(r#"{{"Node":{{"Name":"{FQDN}"}}}}"#).as_bytes()).is_err());
    }

    #[test]
    fn ownership_entropy_failure_precedes_side_effects() {
        let error = generate_token_with_entropy(&FixedEntropy(Err("injected entropy failure")))
            .unwrap_err();
        assert!(error.to_string().contains("injected entropy failure"));

        let runner = FakeRunner::default();
        let adapter = TailscaleServeAdapter::with_runner("tailscale", runner);
        assert!(adapter.runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    #[ignore = "subprocess helper for the native command timeout assertion"]
    fn native_runner_sleep_helper() {
        std::thread::sleep(Duration::from_secs(2));
    }
}

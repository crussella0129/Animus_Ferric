//! Bounded, platform-native access to the Tailscale LocalAPI.
//!
//! Tailscale v1.102.2 serves HTTP/1.1 over a Unix-domain socket on Unix and a
//! named pipe on Windows. The HTTP authority remains `local-tailscaled.sock`
//! on every transport. Sandboxed macOS GUI variants use a separate
//! localhost-port-and-token discovery mechanism and are deliberately not
//! implemented here. The production default therefore reports unsupported on
//! macOS; only the dedicated lifecycle-test binary can bypass that default.
//!
//! Windows timeout safety uses heap-owned OVERLAPPED state and an internal
//! bounded buffer. If `CancelIoEx` has not completed by the absolute deadline,
//! the pipe is closed and poisoned and that one pending state (at most one per
//! session, including its event handle) is retained until process exit. This
//! keeps return latency bounded and prevents late kernel access to caller or
//! freed memory; the OS reclaims the retained allocation and handle at exit.

use std::collections::HashSet;
use std::fmt;
#[cfg(any(
    all(unix, not(target_os = "macos")),
    test,
    feature = "lifecycle-fixture"
))]
use std::io::Read;
use std::io::{self, Write};
use std::time::{Duration, Instant};

#[cfg(any(test, feature = "lifecycle-fixture"))]
use std::net::{SocketAddr, TcpStream};
#[cfg(all(unix, not(target_os = "macos")))]
use std::path::{Path, PathBuf};

use serde::de::{DeserializeSeed, Error as _, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use sha2::{Digest, Sha256};

const LOCALAPI_HOST: &str = "local-tailscaled.sock";
const TAILSCALE_CAPABILITY_VERSION: u16 = 142;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const HEADER_LIMIT: usize = 32 * 1024;
const BODY_LIMIT: usize = 256 * 1024;
const CHUNK_LINE_LIMIT: usize = 1024;
const IO_BUFFER_SIZE: usize = 8 * 1024;
const ETAG_HEX_LEN: usize = 64;
const PINNED_TAILSCALE_VERSION: &str = "1.102.2";

#[cfg(windows)]
const DEFAULT_WINDOWS_PIPE: &str = r"\\.\pipe\ProtectedPrefix\Administrators\Tailscale\tailscaled";

#[cfg(all(unix, not(target_os = "macos")))]
const DEFAULT_UNIX_SOCKET: &str = "/var/run/tailscale/tailscaled.sock";

/// Test-only endpoint override honored only by the dedicated
/// `ferric-lifecycle-test` binary. The ordinary `ferric` binary ignores it,
/// including in `--all-features` builds. The value must be a numeric loopback
/// `SocketAddr` with a nonzero port.
#[cfg(feature = "lifecycle-fixture")]
pub const TEST_TCP_ENDPOINT_ENV: &str = "FERRIC_TAILSCALE_LOCALAPI_TEST_TCP";

/// Errors intentionally omit LocalAPI response bodies because status and Serve
/// configuration can contain machine and tailnet identity.
#[derive(Debug)]
pub enum LocalApiError {
    #[cfg(any(target_os = "macos", not(any(unix, windows))))]
    UnsupportedPlatform,
    InvalidEndpoint(String),
    Timeout,
    Io(io::Error),
    HeaderTooLarge,
    BodyTooLarge,
    Protocol(&'static str),
    InvalidJson(serde_json::Error),
    InvalidEtag,
    IncompatibleDaemon(&'static str),
    MissingStatusField(&'static str),
    AccessDenied(u16),
    HttpStatus(u16),
    ConnectionNotReusable,
}

/// A failed Serve-config compare-and-set operation, classified by whether the
/// caller can safely retry with the same precondition.
#[derive(Debug)]
pub enum ServeConfigCasError {
    /// Validation or connection setup failed before any POST bytes were sent.
    NoMutation(LocalApiError),
    /// tailscaled returned HTTP 412; the supplied ETag was stale and no update
    /// was applied.
    PreconditionFailed,
    /// No response proving a no-op was obtained. This includes I/O/protocol
    /// failure after POST began and daemon errors that can follow persistence.
    /// The update might have committed; callers must re-read state.
    Indeterminate(LocalApiError),
}

impl fmt::Display for LocalApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(any(target_os = "macos", not(any(unix, windows))))]
            Self::UnsupportedPlatform => {
                f.write_str("the Tailscale LocalAPI transport is unsupported on this platform")
            }
            Self::InvalidEndpoint(detail) => write!(f, "invalid LocalAPI endpoint: {detail}"),
            Self::Timeout => f.write_str("the Tailscale LocalAPI request timed out"),
            Self::Io(error) => write!(f, "Tailscale LocalAPI I/O failed: {error}"),
            Self::HeaderTooLarge => f.write_str("the Tailscale LocalAPI headers exceeded the cap"),
            Self::BodyTooLarge => f.write_str("the Tailscale LocalAPI body exceeded the cap"),
            Self::Protocol(detail) => {
                write!(f, "the Tailscale LocalAPI returned invalid HTTP: {detail}")
            }
            Self::InvalidJson(error) => {
                write!(f, "the Tailscale LocalAPI returned invalid JSON: {error}")
            }
            Self::InvalidEtag => {
                f.write_str("the Tailscale LocalAPI returned an invalid Serve-config ETag")
            }
            Self::IncompatibleDaemon(detail) => {
                write!(f, "the Tailscale LocalAPI daemon is incompatible: {detail}")
            }
            Self::MissingStatusField(field) => {
                write!(f, "the Tailscale LocalAPI status omitted {field}")
            }
            Self::AccessDenied(status) => {
                write!(f, "Tailscale LocalAPI access was denied (HTTP {status})")
            }
            Self::HttpStatus(status) => {
                write!(f, "the Tailscale LocalAPI returned HTTP {status}")
            }
            Self::ConnectionNotReusable => f.write_str(
                "the Tailscale LocalAPI closed the connection before the daemon snapshot completed",
            ),
        }
    }
}

impl std::error::Error for LocalApiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InvalidJson(error) => Some(error),
            _ => None,
        }
    }
}

impl fmt::Display for ServeConfigCasError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoMutation(error) => write!(f, "Serve-config update made no mutation: {error}"),
            Self::PreconditionFailed => {
                f.write_str("the Tailscale Serve configuration changed concurrently")
            }
            Self::Indeterminate(error) => write!(
                f,
                "Serve-config update might have committed; re-read daemon state: {error}"
            ),
        }
    }
}

impl std::error::Error for ServeConfigCasError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoMutation(error) | Self::Indeterminate(error) => Some(error),
            Self::PreconditionFailed => None,
        }
    }
}

impl From<io::Error> for LocalApiError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for LocalApiError {
    fn from(error: serde_json::Error) -> Self {
        Self::InvalidJson(error)
    }
}

#[derive(Debug, Clone)]
enum Endpoint {
    #[cfg(feature = "lifecycle-fixture")]
    Invalid(String),
    #[cfg(any(target_os = "macos", not(any(unix, windows))))]
    Unsupported,
    #[cfg(all(unix, not(target_os = "macos")))]
    Unix(PathBuf),
    #[cfg(windows)]
    NamedPipe(std::ffi::OsString),
    #[cfg(any(test, feature = "lifecycle-fixture"))]
    Tcp(SocketAddr),
}

/// A reusable LocalAPI client. Each high-level operation has one absolute
/// five-second deadline covering connect, request writes, header parsing, and
/// the bounded response body.
#[derive(Debug, Clone)]
pub struct TailscaleLocalApiClient {
    endpoint: Endpoint,
}

#[derive(Debug, Clone, Copy)]
enum DaemonCompatibility {
    ExactV1_102_2,
    CleanupVersionDrift,
}

impl Default for TailscaleLocalApiClient {
    fn default() -> Self {
        Self::new()
    }
}

impl TailscaleLocalApiClient {
    pub fn new() -> Self {
        #[cfg(feature = "lifecycle-fixture")]
        if env!("CARGO_BIN_NAME") == "ferric-lifecycle-test"
            && let Some(raw) = std::env::var_os(TEST_TCP_ENDPOINT_ENV)
        {
            return match parse_test_tcp_endpoint(&raw.to_string_lossy()) {
                Ok(address) => Self {
                    endpoint: Endpoint::Tcp(address),
                },
                Err(detail) => Self {
                    endpoint: Endpoint::Invalid(detail),
                },
            };
        }

        #[cfg(all(unix, not(target_os = "macos")))]
        {
            Self {
                endpoint: Endpoint::Unix(PathBuf::from(DEFAULT_UNIX_SOCKET)),
            }
        }

        #[cfg(target_os = "macos")]
        {
            Self {
                endpoint: Endpoint::Unsupported,
            }
        }

        #[cfg(windows)]
        {
            Self {
                endpoint: Endpoint::NamedPipe(DEFAULT_WINDOWS_PIPE.into()),
            }
        }

        #[cfg(not(any(unix, windows)))]
        {
            Self {
                endpoint: Endpoint::Unsupported,
            }
        }
    }

    /// Opens a same-connection session. Callers that need to bind status,
    /// Serve pre-state, and a CAS update to one accepted daemon connection
    /// should use this API rather than the convenience methods below.
    pub fn open_session(&self) -> Result<LocalApiSession, LocalApiError> {
        self.open_session_with(DaemonCompatibility::ExactV1_102_2)
    }

    fn open_session_with(
        &self,
        compatibility: DaemonCompatibility,
    ) -> Result<LocalApiSession, LocalApiError> {
        let deadline = absolute_deadline();
        self.open_session_with_deadline(compatibility, deadline)
    }

    fn open_session_with_deadline(
        &self,
        compatibility: DaemonCompatibility,
        deadline: Instant,
    ) -> Result<LocalApiSession, LocalApiError> {
        let connection = Connection::connect(&self.endpoint, deadline, compatibility)?;
        Ok(LocalApiSession {
            connection,
            deadline,
        })
    }

    /// Reopens a session rejected solely for exact-version incompatibility
    /// without resetting its absolute deadline or changing its endpoint.
    pub(crate) fn reopen_cleanup_session(
        &self,
        rejected: LocalApiSession,
    ) -> Result<LocalApiSession, LocalApiError> {
        let deadline = rejected.deadline;
        drop(rejected);
        self.open_session_with_deadline(DaemonCompatibility::CleanupVersionDrift, deadline)
    }

    pub fn get_status(&self) -> Result<LocalApiStatus, LocalApiError> {
        self.open_session()?.get_status()
    }

    #[allow(dead_code)]
    #[cfg(test)]
    pub fn compare_and_set_serve_config(
        &self,
        etag: &str,
        config_json: &[u8],
    ) -> Result<(), ServeConfigCasError> {
        validate_cas_input(etag, config_json).map_err(ServeConfigCasError::NoMutation)?;
        self.open_session()
            .map_err(ServeConfigCasError::NoMutation)?
            .compare_and_set_serve_config(etag, config_json)
    }

    #[cfg(test)]
    pub fn with_test_tcp(address: SocketAddr) -> Result<Self, LocalApiError> {
        validate_loopback_address(address)?;
        Ok(Self {
            endpoint: Endpoint::Tcp(address),
        })
    }
}

#[cfg(feature = "lifecycle-fixture")]
fn parse_test_tcp_endpoint(raw: &str) -> Result<SocketAddr, String> {
    let address = raw
        .parse::<SocketAddr>()
        .map_err(|_| "test TCP endpoint must be a numeric socket address".to_string())?;
    if !address.ip().is_loopback() || address.port() == 0 {
        return Err("test TCP endpoint must be loopback with a nonzero port".to_string());
    }
    Ok(address)
}

#[cfg(any(test, feature = "lifecycle-fixture"))]
fn validate_loopback_address(address: SocketAddr) -> Result<(), LocalApiError> {
    if !address.ip().is_loopback() || address.port() == 0 {
        return Err(LocalApiError::InvalidEndpoint(
            "test TCP endpoint must be loopback with a nonzero port".to_string(),
        ));
    }
    Ok(())
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalApiStatus {
    raw_json: Vec<u8>,
    backend_state: String,
    self_dns_name: String,
    self_stable_id: String,
    self_node_id: Option<String>,
    https_capable: bool,
    cert_domains: Vec<String>,
}

#[allow(dead_code)]
impl LocalApiStatus {
    pub fn raw_json(&self) -> &[u8] {
        &self.raw_json
    }

    pub fn backend_state(&self) -> &str {
        &self.backend_state
    }

    /// Returns the status `Self.DNSName` exactly as sent by tailscaled. In
    /// v1.102.2 this is an FQDN with a trailing dot.
    pub fn self_dns_name(&self) -> &str {
        &self.self_dns_name
    }

    /// Returns status `Self.ID`, whose v1.102.2 type is `StableNodeID`.
    pub fn self_stable_id(&self) -> &str {
        &self.self_stable_id
    }

    /// Returns status `Self.NodeID` in a lossless scalar string form when the
    /// daemon supplies it.
    pub fn self_node_id(&self) -> Option<&str> {
        self.self_node_id.as_deref()
    }

    pub fn https_capable(&self) -> bool {
        self.https_capable
    }

    /// Certificate names tailscaled is willing to provision. Tailscale status
    /// reports these as FQDNs without a trailing dot.
    pub fn cert_domains(&self) -> &[String] {
        &self.cert_domains
    }

    pub fn has_cert_domain(&self, dotless_fqdn: &str) -> bool {
        !dotless_fqdn.is_empty()
            && !dotless_fqdn.ends_with('.')
            && self.cert_domains.iter().any(|name| name == dotless_fqdn)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeConfigSnapshot {
    etag: String,
    raw_json: Vec<u8>,
}

impl ServeConfigSnapshot {
    pub fn etag(&self) -> &str {
        &self.etag
    }

    pub fn raw_json(&self) -> &[u8] {
        &self.raw_json
    }
}

/// One connected LocalAPI stream and one absolute deadline. HTTP keep-alive is
/// intentional: sequential reads on this object cannot cross a daemon restart
/// without observing EOF and failing closed.
pub struct LocalApiSession {
    connection: Connection,
    deadline: Instant,
}

impl LocalApiSession {
    pub fn get_status(&mut self) -> Result<LocalApiStatus, LocalApiError> {
        let response = self.connection.request(
            "GET",
            "/localapi/v0/status?peers=false",
            &[],
            &[],
            self.deadline,
        )?;
        require_success(&response)?;
        require_json_content_type(&response)?;
        parse_status(response.body)
    }

    pub fn get_serve_config(&mut self) -> Result<ServeConfigSnapshot, LocalApiError> {
        let response =
            self.connection
                .request("GET", "/localapi/v0/serve-config", &[], &[], self.deadline)?;
        require_success(&response)?;
        require_json_content_type(&response)?;
        let etag = response
            .unique_header("etag")?
            .ok_or(LocalApiError::InvalidEtag)?;
        validate_serve_config_etag(etag, &response.body)?;
        let _ = parse_json_no_duplicates(&response.body)?;
        Ok(ServeConfigSnapshot {
            etag: etag.to_string(),
            raw_json: response.body,
        })
    }

    pub fn compare_and_set_serve_config(
        &mut self,
        etag: &str,
        config_json: &[u8],
    ) -> Result<(), ServeConfigCasError> {
        validate_cas_input(etag, config_json).map_err(ServeConfigCasError::NoMutation)?;
        if !self.connection.is_reusable() {
            return Err(ServeConfigCasError::NoMutation(
                LocalApiError::ConnectionNotReusable,
            ));
        }
        let response = self
            .connection
            .request(
                "POST",
                "/localapi/v0/serve-config",
                &[("If-Match", etag), ("Content-Type", "application/json")],
                config_json,
                self.deadline,
            )
            .map_err(ServeConfigCasError::Indeterminate)?;
        match response.status {
            200 => Ok(()),
            401 | 403 => Err(ServeConfigCasError::Indeterminate(
                LocalApiError::AccessDenied(response.status),
            )),
            412 => Err(ServeConfigCasError::PreconditionFailed),
            status => Err(ServeConfigCasError::Indeterminate(
                LocalApiError::HttpStatus(status),
            )),
        }
    }
}

fn validate_cas_input(etag: &str, config_json: &[u8]) -> Result<(), LocalApiError> {
    validate_etag(etag)?;
    if config_json.len() > BODY_LIMIT {
        return Err(LocalApiError::BodyTooLarge);
    }
    let _ = parse_json_no_duplicates(config_json)?;
    Ok(())
}

fn absolute_deadline() -> Instant {
    Instant::now()
        .checked_add(REQUEST_TIMEOUT)
        .unwrap_or_else(Instant::now)
}

fn require_success(response: &HttpResponse) -> Result<(), LocalApiError> {
    match response.status {
        200 => Ok(()),
        401 | 403 => Err(LocalApiError::AccessDenied(response.status)),
        status => Err(LocalApiError::HttpStatus(status)),
    }
}

fn require_json_content_type(response: &HttpResponse) -> Result<(), LocalApiError> {
    let content_type = response
        .unique_header("content-type")?
        .ok_or(LocalApiError::Protocol("missing Content-Type"))?;
    let media_type = content_type
        .split_once(';')
        .map_or(content_type, |(media_type, _)| media_type)
        .trim();
    if !media_type.eq_ignore_ascii_case("application/json") {
        return Err(LocalApiError::Protocol("unexpected Content-Type"));
    }
    Ok(())
}

fn validate_etag(etag: &str) -> Result<(), LocalApiError> {
    if etag.len() != ETAG_HEX_LEN
        || !etag
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(LocalApiError::InvalidEtag);
    }
    Ok(())
}

fn validate_serve_config_etag(etag: &str, body: &[u8]) -> Result<(), LocalApiError> {
    validate_etag(etag)?;
    let digest = Sha256::digest(body);
    let matches = etag
        .as_bytes()
        .chunks_exact(2)
        .zip(digest.iter())
        .all(|(pair, expected)| {
            let high = lower_hex_value(pair[0]);
            let low = lower_hex_value(pair[1]);
            high.zip(low)
                .is_some_and(|(high, low)| high << 4 | low == *expected)
        });
    if !matches {
        return Err(LocalApiError::InvalidEtag);
    }
    Ok(())
}

fn lower_hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn parse_status(raw_json: Vec<u8>) -> Result<LocalApiStatus, LocalApiError> {
    let value = parse_json_no_duplicates(&raw_json)?;
    let root = value
        .as_object()
        .ok_or(LocalApiError::MissingStatusField("the status object"))?;
    let backend_state = required_string(root.get("BackendState"), "BackendState")?;
    let self_status = root
        .get("Self")
        .and_then(Value::as_object)
        .ok_or(LocalApiError::MissingStatusField("Self"))?;
    let self_dns_name = required_string(self_status.get("DNSName"), "Self.DNSName")?;
    if self_dns_name == "." || !self_dns_name.ends_with('.') {
        return Err(LocalApiError::MissingStatusField(
            "trailing-dot Self.DNSName",
        ));
    }
    let self_stable_id = required_string(self_status.get("ID"), "Self.ID")?;
    let self_node_id = self_status
        .get("NodeID")
        .map(scalar_id)
        .transpose()?
        .filter(|id| !id.is_empty());
    let https_capable = match self_status.get("CapMap") {
        None | Some(Value::Null) => false,
        Some(Value::Object(capabilities)) => capabilities.contains_key("https"),
        Some(_) => return Err(LocalApiError::MissingStatusField("Self.CapMap object")),
    };
    let cert_domains = parse_cert_domains(root.get("CertDomains"))?;
    Ok(LocalApiStatus {
        raw_json,
        backend_state,
        self_dns_name,
        self_stable_id,
        self_node_id,
        https_capable,
        cert_domains,
    })
}

fn parse_cert_domains(value: Option<&Value>) -> Result<Vec<String>, LocalApiError> {
    let values = match value {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(values)) => values,
        Some(_) => return Err(LocalApiError::MissingStatusField("CertDomains array")),
    };
    let mut domains = Vec::with_capacity(values.len());
    let mut seen = HashSet::with_capacity(values.len());
    for value in values {
        let domain = value
            .as_str()
            .filter(|domain| !domain.is_empty() && !domain.ends_with('.'))
            .ok_or(LocalApiError::MissingStatusField(
                "dotless CertDomains entry",
            ))?;
        if !seen.insert(domain) {
            return Err(LocalApiError::MissingStatusField(
                "unique CertDomains entries",
            ));
        }
        domains.push(domain.to_string());
    }
    Ok(domains)
}

/// serde_json's ordinary `Value` deserializer accepts duplicate object keys
/// with last-key-wins semantics. Identity and Serve configuration are
/// security-sensitive, so reject duplicates recursively instead.
fn parse_json_no_duplicates(bytes: &[u8]) -> Result<Value, LocalApiError> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = DuplicateSafeValueSeed.deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(value)
}

#[derive(Clone, Copy)]
struct DuplicateSafeValueSeed;

impl<'de> DeserializeSeed<'de> for DuplicateSafeValueSeed {
    type Value = Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(DuplicateSafeValueVisitor)
    }
}

struct DuplicateSafeValueVisitor;

impl<'de> Visitor<'de> for DuplicateSafeValueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value without duplicate object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(Value::String(value))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        DuplicateSafeValueSeed.deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::with_capacity(sequence.size_hint().unwrap_or(0));
        while let Some(value) = sequence.next_element_seed(DuplicateSafeValueSeed)? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = serde_json::Map::with_capacity(object.size_hint().unwrap_or(0));
        while let Some(key) = object.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(A::Error::custom("duplicate JSON object key"));
            }
            let value = object.next_value_seed(DuplicateSafeValueSeed)?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

fn required_string(value: Option<&Value>, field: &'static str) -> Result<String, LocalApiError> {
    let value = value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(LocalApiError::MissingStatusField(field))?;
    Ok(value.to_string())
}

fn scalar_id(value: &Value) -> Result<String, LocalApiError> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Null => Ok(String::new()),
        _ => Err(LocalApiError::MissingStatusField("scalar Self.NodeID")),
    }
}

struct Connection {
    stream: TransportStream,
    buffered: ReadBuffer,
    reusable: bool,
    compatibility: DaemonCompatibility,
}

impl Connection {
    fn connect(
        endpoint: &Endpoint,
        deadline: Instant,
        compatibility: DaemonCompatibility,
    ) -> Result<Self, LocalApiError> {
        let stream = TransportStream::connect(endpoint, deadline)?;
        Ok(Self {
            stream,
            buffered: ReadBuffer::default(),
            reusable: true,
            compatibility,
        })
    }

    fn request(
        &mut self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        deadline: Instant,
    ) -> Result<HttpResponse, LocalApiError> {
        if !self.reusable {
            return Err(LocalApiError::ConnectionNotReusable);
        }
        if body.len() > BODY_LIMIT {
            return Err(LocalApiError::BodyTooLarge);
        }
        let request = encode_request(method, path, headers, body)?;
        if let Err(error) = self.stream.write_all_deadline(&request, deadline) {
            self.reusable = false;
            return Err(error);
        }
        let response = match read_response(&mut self.stream, &mut self.buffered, deadline) {
            Ok(response) => response,
            Err(error) => {
                self.reusable = false;
                return Err(error);
            }
        };
        if let Err(error) = validate_daemon_headers(&response, self.compatibility) {
            self.reusable = false;
            return Err(error);
        }
        self.reusable = !response.connection_close;
        Ok(response)
    }

    fn is_reusable(&self) -> bool {
        self.reusable
    }
}

fn validate_daemon_headers(
    response: &HttpResponse,
    compatibility: DaemonCompatibility,
) -> Result<(), LocalApiError> {
    let capability =
        response
            .unique_header("tailscale-cap")?
            .ok_or(LocalApiError::IncompatibleDaemon(
                "missing Tailscale-Cap response header",
            ))?;
    let capability_valid = match compatibility {
        DaemonCompatibility::ExactV1_102_2 => capability == "142",
        DaemonCompatibility::CleanupVersionDrift => capability
            .parse::<u32>()
            .is_ok_and(|capability| capability >= u32::from(TAILSCALE_CAPABILITY_VERSION)),
    };
    if !capability_valid {
        return Err(LocalApiError::IncompatibleDaemon(
            "unexpected Tailscale-Cap response header",
        ));
    }

    let version =
        response
            .unique_header("tailscale-version")?
            .ok_or(LocalApiError::IncompatibleDaemon(
                "missing Tailscale-Version response header",
            ))?;
    let version_valid = match compatibility {
        DaemonCompatibility::ExactV1_102_2 => has_pinned_semver_core(version),
        DaemonCompatibility::CleanupVersionDrift => parse_sane_semver_core(version)
            .is_some_and(|(major, minor, patch)| major == 1 && (minor, patch) >= (102, 2)),
    };
    if !version_valid {
        return Err(LocalApiError::IncompatibleDaemon(
            "unexpected Tailscale-Version response header",
        ));
    }
    Ok(())
}

fn has_pinned_semver_core(version: &str) -> bool {
    if !has_sane_semver_version(version) {
        return false;
    }
    let Some(suffix) = version.strip_prefix(PINNED_TAILSCALE_VERSION) else {
        return false;
    };
    if suffix.is_empty() {
        return true;
    }
    let bytes = suffix.as_bytes();
    matches!(bytes.first(), Some(b'-' | b'+'))
        && bytes.len() > 1
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'+'))
}

fn has_sane_semver_version(version: &str) -> bool {
    parse_sane_semver_core(version).is_some()
}

fn parse_sane_semver_core(version: &str) -> Option<(u64, u64, u64)> {
    if version.is_empty()
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
    {
        return None;
    }
    let suffix_start = version
        .bytes()
        .position(|byte| matches!(byte, b'-' | b'+'))
        .unwrap_or(version.len());
    let (core, suffix) = version.split_at(suffix_start);
    let parse_component = |component: &str| {
        if component.is_empty()
            || !component.bytes().all(|byte| byte.is_ascii_digit())
            || (component.len() > 1 && component.starts_with('0'))
        {
            None
        } else {
            component.parse::<u64>().ok()
        }
    };
    let mut components = core.split('.');
    let major = parse_component(components.next()?)?;
    let minor = parse_component(components.next()?)?;
    let patch = parse_component(components.next()?)?;
    if components.next().is_some()
        || !(suffix.is_empty()
            || (suffix.len() > 1
                && matches!(suffix.as_bytes().first(), Some(b'-' | b'+'))
                && !suffix.ends_with('.')
                && !suffix.contains("..")
                && suffix.matches('+').count() <= 1))
    {
        return None;
    }
    Some((major, minor, patch))
}

fn encode_request(
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Result<Vec<u8>, LocalApiError> {
    if !matches!(method, "GET" | "POST") || !path.starts_with('/') {
        return Err(LocalApiError::Protocol("invalid request target"));
    }
    let mut request = Vec::with_capacity(256 + body.len());
    write!(request, "{method} {path} HTTP/1.1\r\n").map_err(LocalApiError::Io)?;
    write!(request, "Host: {LOCALAPI_HOST}\r\n").map_err(LocalApiError::Io)?;
    write!(
        request,
        "Tailscale-Cap: {TAILSCALE_CAPABILITY_VERSION}\r\nAccept: application/json\r\nConnection: keep-alive\r\n"
    )
    .map_err(LocalApiError::Io)?;
    for (name, value) in headers {
        if !valid_header_name(name.as_bytes())
            || value
                .as_bytes()
                .iter()
                .any(|byte| matches!(byte, b'\r' | b'\n' | 0))
        {
            return Err(LocalApiError::Protocol("invalid request header"));
        }
        write!(request, "{name}: {value}\r\n").map_err(LocalApiError::Io)?;
    }
    if !body.is_empty() || method == "POST" {
        write!(request, "Content-Length: {}\r\n", body.len()).map_err(LocalApiError::Io)?;
    }
    request.extend_from_slice(b"\r\n");
    request.extend_from_slice(body);
    Ok(request)
}

#[derive(Debug)]
struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    connection_close: bool,
}

impl HttpResponse {
    fn unique_header(&self, name: &str) -> Result<Option<&str>, LocalApiError> {
        let mut values = self
            .headers
            .iter()
            .filter(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str());
        let first = values.next();
        if values.next().is_some() {
            return Err(LocalApiError::Protocol("duplicate singleton header"));
        }
        Ok(first)
    }
}

#[derive(Default)]
struct ReadBuffer {
    bytes: Vec<u8>,
    position: usize,
}

impl ReadBuffer {
    fn available(&self) -> &[u8] {
        &self.bytes[self.position..]
    }

    fn consume(&mut self, count: usize) {
        self.position += count;
        if self.position == self.bytes.len() {
            self.bytes.clear();
            self.position = 0;
        } else if self.position > IO_BUFFER_SIZE && self.position * 2 > self.bytes.len() {
            self.bytes.drain(..self.position);
            self.position = 0;
        }
    }

    fn extend(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }
}

fn read_response(
    stream: &mut TransportStream,
    buffered: &mut ReadBuffer,
    deadline: Instant,
) -> Result<HttpResponse, LocalApiError> {
    let header_end = loop {
        if let Some(index) = find_bytes(buffered.available(), b"\r\n\r\n") {
            break index + 4;
        }
        if buffered.available().len() >= HEADER_LIMIT {
            return Err(LocalApiError::HeaderTooLarge);
        }
        read_more(stream, buffered, deadline)?;
    };
    if header_end > HEADER_LIMIT {
        return Err(LocalApiError::HeaderTooLarge);
    }
    let header_block = buffered.available()[..header_end - 4].to_vec();
    buffered.consume(header_end);
    let (status, headers) = parse_header_block(&header_block)?;
    if status < 200 {
        return Err(LocalApiError::Protocol(
            "informational responses are not supported",
        ));
    }

    let content_length = singleton_header(&headers, "content-length")?
        .map(parse_content_length)
        .transpose()?;
    let transfer_encoding = singleton_header(&headers, "transfer-encoding")?;
    if content_length.is_some() && transfer_encoding.is_some() {
        return Err(LocalApiError::Protocol(
            "Content-Length and Transfer-Encoding were both present",
        ));
    }
    let connection_close = header_has_token(&headers, "connection", "close")?;
    let body_forbidden = matches!(status, 204 | 304);
    if body_forbidden
        && (transfer_encoding.is_some() || content_length.is_some_and(|length| length != 0))
    {
        return Err(LocalApiError::Protocol(
            "body framing was present on a bodyless response",
        ));
    }
    let body = if body_forbidden {
        Vec::new()
    } else if let Some(length) = content_length {
        read_exact_body(stream, buffered, length, deadline)?
    } else if let Some(encoding) = transfer_encoding {
        if !encoding.trim().eq_ignore_ascii_case("chunked") {
            return Err(LocalApiError::Protocol("unsupported Transfer-Encoding"));
        }
        read_chunked_body(stream, buffered, deadline)?
    } else if connection_close {
        read_close_delimited_body(stream, buffered, deadline)?
    } else {
        return Err(LocalApiError::Protocol("response body had no framing"));
    };

    Ok(HttpResponse {
        status,
        headers,
        body,
        connection_close,
    })
}

fn parse_header_block(block: &[u8]) -> Result<(u16, Vec<(String, String)>), LocalApiError> {
    if block.contains(&0) {
        return Err(LocalApiError::Protocol("NUL in response headers"));
    }
    let text = std::str::from_utf8(block)
        .map_err(|_| LocalApiError::Protocol("non-ASCII response headers"))?;
    let mut lines = text.split("\r\n");
    let status_line = lines
        .next()
        .ok_or(LocalApiError::Protocol("missing status line"))?;
    let mut status_parts = status_line.splitn(3, ' ');
    if status_parts.next() != Some("HTTP/1.1") {
        return Err(LocalApiError::Protocol("unsupported HTTP version"));
    }
    let status_text = status_parts
        .next()
        .ok_or(LocalApiError::Protocol("missing response status"))?;
    if status_text.len() != 3 || !status_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(LocalApiError::Protocol("invalid response status"));
    }
    let status = status_text
        .parse::<u16>()
        .map_err(|_| LocalApiError::Protocol("invalid response status"))?;

    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() || line.starts_with([' ', '\t']) {
            return Err(LocalApiError::Protocol("invalid response header line"));
        }
        let (name, value) = line
            .split_once(':')
            .ok_or(LocalApiError::Protocol("malformed response header"))?;
        if !valid_header_name(name.as_bytes()) {
            return Err(LocalApiError::Protocol("invalid response header name"));
        }
        let value = value.trim_matches([' ', '\t']);
        if value
            .as_bytes()
            .iter()
            .any(|byte| *byte < b' ' && *byte != b'\t' || *byte == 0x7f)
        {
            return Err(LocalApiError::Protocol("invalid response header value"));
        }
        headers.push((name.to_ascii_lowercase(), value.to_string()));
    }
    Ok((status, headers))
}

fn valid_header_name(name: &[u8]) -> bool {
    !name.is_empty()
        && name.iter().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

fn singleton_header<'a>(
    headers: &'a [(String, String)],
    name: &str,
) -> Result<Option<&'a str>, LocalApiError> {
    let mut values = headers
        .iter()
        .filter(|(header_name, _)| header_name == name)
        .map(|(_, value)| value.as_str());
    let first = values.next();
    if values.next().is_some() {
        return Err(LocalApiError::Protocol("duplicate framing header"));
    }
    Ok(first)
}

fn header_has_token(
    headers: &[(String, String)],
    name: &str,
    token: &str,
) -> Result<bool, LocalApiError> {
    let mut found = false;
    for (_, value) in headers
        .iter()
        .filter(|(header_name, _)| header_name == name)
    {
        for candidate in value.split(',') {
            let candidate = candidate.trim();
            if candidate.is_empty() {
                return Err(LocalApiError::Protocol("empty header token"));
            }
            found |= candidate.eq_ignore_ascii_case(token);
        }
    }
    Ok(found)
}

fn parse_content_length(value: &str) -> Result<usize, LocalApiError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(LocalApiError::Protocol("invalid Content-Length"));
    }
    let length = value
        .parse::<usize>()
        .map_err(|_| LocalApiError::BodyTooLarge)?;
    if length > BODY_LIMIT {
        return Err(LocalApiError::BodyTooLarge);
    }
    Ok(length)
}

fn read_exact_body(
    stream: &mut TransportStream,
    buffered: &mut ReadBuffer,
    length: usize,
    deadline: Instant,
) -> Result<Vec<u8>, LocalApiError> {
    if length > BODY_LIMIT {
        return Err(LocalApiError::BodyTooLarge);
    }
    let mut body = Vec::with_capacity(length);
    while body.len() < length {
        if buffered.available().is_empty() {
            read_more(stream, buffered, deadline)?;
        }
        let count = (length - body.len()).min(buffered.available().len());
        body.extend_from_slice(&buffered.available()[..count]);
        buffered.consume(count);
    }
    Ok(body)
}

fn read_chunked_body(
    stream: &mut TransportStream,
    buffered: &mut ReadBuffer,
    deadline: Instant,
) -> Result<Vec<u8>, LocalApiError> {
    let mut body = Vec::new();
    loop {
        let size_line = read_crlf_line(stream, buffered, deadline, CHUNK_LINE_LIMIT)?;
        if size_line.is_empty() || size_line.contains(&b';') {
            return Err(LocalApiError::Protocol("invalid chunk-size line"));
        }
        let size_text = std::str::from_utf8(&size_line)
            .map_err(|_| LocalApiError::Protocol("non-ASCII chunk size"))?;
        if !size_text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(LocalApiError::Protocol("invalid chunk size"));
        }
        let size = usize::from_str_radix(size_text, 16).map_err(|_| LocalApiError::BodyTooLarge)?;
        if size > BODY_LIMIT.saturating_sub(body.len()) {
            return Err(LocalApiError::BodyTooLarge);
        }
        if size == 0 {
            let trailer = read_crlf_line(stream, buffered, deadline, CHUNK_LINE_LIMIT)?;
            if !trailer.is_empty() {
                return Err(LocalApiError::Protocol("chunk trailers are not supported"));
            }
            return Ok(body);
        }
        let chunk = read_exact_body(stream, buffered, size, deadline)?;
        body.extend_from_slice(&chunk);
        let terminator = read_exact_body(stream, buffered, 2, deadline)?;
        if terminator != b"\r\n" {
            return Err(LocalApiError::Protocol("invalid chunk terminator"));
        }
    }
}

fn read_crlf_line(
    stream: &mut TransportStream,
    buffered: &mut ReadBuffer,
    deadline: Instant,
    limit: usize,
) -> Result<Vec<u8>, LocalApiError> {
    loop {
        if let Some(index) = find_bytes(buffered.available(), b"\r\n") {
            if index > limit {
                return Err(LocalApiError::HeaderTooLarge);
            }
            let line = buffered.available()[..index].to_vec();
            buffered.consume(index + 2);
            return Ok(line);
        }
        if buffered.available().len() > limit {
            return Err(LocalApiError::HeaderTooLarge);
        }
        read_more(stream, buffered, deadline)?;
    }
}

fn read_close_delimited_body(
    stream: &mut TransportStream,
    buffered: &mut ReadBuffer,
    deadline: Instant,
) -> Result<Vec<u8>, LocalApiError> {
    let mut body = buffered.available().to_vec();
    buffered.consume(buffered.available().len());
    if body.len() > BODY_LIMIT {
        return Err(LocalApiError::BodyTooLarge);
    }
    let mut chunk = [0_u8; IO_BUFFER_SIZE];
    loop {
        let count = stream.read_deadline(&mut chunk, deadline)?;
        if count == 0 {
            return Ok(body);
        }
        if count > BODY_LIMIT.saturating_sub(body.len()) {
            return Err(LocalApiError::BodyTooLarge);
        }
        body.extend_from_slice(&chunk[..count]);
    }
}

fn read_more(
    stream: &mut TransportStream,
    buffered: &mut ReadBuffer,
    deadline: Instant,
) -> Result<(), LocalApiError> {
    let mut chunk = [0_u8; IO_BUFFER_SIZE];
    let count = stream.read_deadline(&mut chunk, deadline)?;
    if count == 0 {
        return Err(LocalApiError::Protocol("unexpected EOF"));
    }
    buffered.extend(&chunk[..count]);
    Ok(())
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

enum TransportStream {
    #[cfg(all(unix, not(target_os = "macos")))]
    Unix(std::os::unix::net::UnixStream),
    #[cfg(windows)]
    NamedPipe(windows_pipe::NamedPipe),
    #[cfg(any(test, feature = "lifecycle-fixture"))]
    Tcp(TcpStream),
    #[cfg(test)]
    Memory(TestStream),
}

impl TransportStream {
    fn connect(endpoint: &Endpoint, deadline: Instant) -> Result<Self, LocalApiError> {
        match endpoint {
            #[cfg(feature = "lifecycle-fixture")]
            Endpoint::Invalid(detail) => Err(LocalApiError::InvalidEndpoint(detail.clone())),
            #[cfg(all(unix, not(target_os = "macos")))]
            Endpoint::Unix(path) => Ok(Self::Unix(connect_unix(path, deadline)?)),
            #[cfg(windows)]
            Endpoint::NamedPipe(path) => Ok(Self::NamedPipe(windows_pipe::NamedPipe::connect(
                path, deadline,
            )?)),
            #[cfg(any(test, feature = "lifecycle-fixture"))]
            Endpoint::Tcp(address) => {
                validate_loopback_address(*address)?;
                let stream = TcpStream::connect_timeout(address, remaining(deadline)?)?;
                stream.set_nodelay(true)?;
                Ok(Self::Tcp(stream))
            }
            #[cfg(any(target_os = "macos", not(any(unix, windows))))]
            Endpoint::Unsupported => Err(LocalApiError::UnsupportedPlatform),
        }
    }

    fn read_deadline(
        &mut self,
        buffer: &mut [u8],
        deadline: Instant,
    ) -> Result<usize, LocalApiError> {
        match self {
            #[cfg(all(unix, not(target_os = "macos")))]
            Self::Unix(stream) => read_socket_deadline(stream, buffer, deadline),
            #[cfg(windows)]
            Self::NamedPipe(stream) => stream.read_deadline(buffer, deadline),
            #[cfg(any(test, feature = "lifecycle-fixture"))]
            Self::Tcp(stream) => read_socket_deadline(stream, buffer, deadline),
            #[cfg(test)]
            Self::Memory(stream) => Ok(stream.read(buffer)?),
        }
    }

    fn write_all_deadline(
        &mut self,
        mut buffer: &[u8],
        deadline: Instant,
    ) -> Result<(), LocalApiError> {
        while !buffer.is_empty() {
            let count = match self {
                #[cfg(all(unix, not(target_os = "macos")))]
                Self::Unix(stream) => write_socket_deadline(stream, buffer, deadline)?,
                #[cfg(windows)]
                Self::NamedPipe(stream) => stream.write_deadline(buffer, deadline)?,
                #[cfg(any(test, feature = "lifecycle-fixture"))]
                Self::Tcp(stream) => write_socket_deadline(stream, buffer, deadline)?,
                #[cfg(test)]
                Self::Memory(stream) => stream.write(buffer)?,
            };
            if count == 0 {
                return Err(LocalApiError::Protocol("zero-length request write"));
            }
            buffer = &buffer[count..];
        }
        Ok(())
    }
}

#[cfg(any(
    all(unix, not(target_os = "macos")),
    test,
    feature = "lifecycle-fixture"
))]
fn read_socket_deadline<S: Read + SocketTimeout>(
    stream: &mut S,
    buffer: &mut [u8],
    deadline: Instant,
) -> Result<usize, LocalApiError> {
    loop {
        stream.set_read_deadline(remaining(deadline)?)?;
        match stream.read(buffer) {
            Ok(count) => return Ok(count),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                return Err(LocalApiError::Timeout);
            }
            Err(error) => return Err(LocalApiError::Io(error)),
        }
    }
}

#[cfg(any(
    all(unix, not(target_os = "macos")),
    test,
    feature = "lifecycle-fixture"
))]
fn write_socket_deadline<S: Write + SocketTimeout>(
    stream: &mut S,
    buffer: &[u8],
    deadline: Instant,
) -> Result<usize, LocalApiError> {
    loop {
        stream.set_write_deadline(remaining(deadline)?)?;
        match stream.write(buffer) {
            Ok(count) => return Ok(count),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                return Err(LocalApiError::Timeout);
            }
            Err(error) => return Err(LocalApiError::Io(error)),
        }
    }
}

#[cfg(any(
    all(unix, not(target_os = "macos")),
    test,
    feature = "lifecycle-fixture"
))]
trait SocketTimeout {
    fn set_read_deadline(&self, timeout: Duration) -> io::Result<()>;
    fn set_write_deadline(&self, timeout: Duration) -> io::Result<()>;
}

#[cfg(any(test, feature = "lifecycle-fixture"))]
impl SocketTimeout for TcpStream {
    fn set_read_deadline(&self, timeout: Duration) -> io::Result<()> {
        self.set_read_timeout(Some(nonzero_timeout(timeout)))
    }

    fn set_write_deadline(&self, timeout: Duration) -> io::Result<()> {
        self.set_write_timeout(Some(nonzero_timeout(timeout)))
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
impl SocketTimeout for std::os::unix::net::UnixStream {
    fn set_read_deadline(&self, timeout: Duration) -> io::Result<()> {
        self.set_read_timeout(Some(nonzero_timeout(timeout)))
    }

    fn set_write_deadline(&self, timeout: Duration) -> io::Result<()> {
        self.set_write_timeout(Some(nonzero_timeout(timeout)))
    }
}

#[cfg(any(
    all(unix, not(target_os = "macos")),
    test,
    feature = "lifecycle-fixture"
))]
fn nonzero_timeout(timeout: Duration) -> Duration {
    timeout.max(Duration::from_millis(1))
}

fn remaining(deadline: Instant) -> Result<Duration, LocalApiError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        Err(LocalApiError::Timeout)
    } else {
        Ok(remaining)
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn connect_unix(
    path: &Path,
    deadline: Instant,
) -> Result<std::os::unix::net::UnixStream, LocalApiError> {
    use std::mem::{offset_of, size_of, zeroed};
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;

    let path = path.as_os_str().as_bytes();
    if path.is_empty() || path.contains(&0) {
        return Err(LocalApiError::InvalidEndpoint(
            "Unix socket path is empty or contains NUL".to_string(),
        ));
    }

    // SAFETY: `socket` returns a new descriptor; OwnedFd immediately assumes
    // ownership on success.
    let raw_fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if raw_fd < 0 {
        return Err(LocalApiError::Io(io::Error::last_os_error()));
    }
    // SAFETY: raw_fd is newly owned and valid here.
    let owned = unsafe { OwnedFd::from_raw_fd(raw_fd) };

    // SAFETY: fcntl operates on the owned descriptor with scalar flags.
    let flags = unsafe { libc::fcntl(raw_fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(raw_fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(LocalApiError::Io(io::Error::last_os_error()));
    }
    // SAFETY: same descriptor and flag-only operation.
    let descriptor_flags = unsafe { libc::fcntl(raw_fd, libc::F_GETFD) };
    if descriptor_flags < 0
        || unsafe { libc::fcntl(raw_fd, libc::F_SETFD, descriptor_flags | libc::FD_CLOEXEC) } < 0
    {
        return Err(LocalApiError::Io(io::Error::last_os_error()));
    }

    // SAFETY: sockaddr_un is a plain-old-data C structure that permits zero
    // initialization.
    let mut address: libc::sockaddr_un = unsafe { zeroed() };
    if path.len() >= address.sun_path.len() {
        return Err(LocalApiError::InvalidEndpoint(
            "Unix socket path exceeds sockaddr_un".to_string(),
        ));
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (destination, source) in address.sun_path.iter_mut().zip(path.iter().copied()) {
        *destination = source as libc::c_char;
    }
    let address_length = offset_of!(libc::sockaddr_un, sun_path) + path.len() + 1;
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    {
        address.sun_len = u8::try_from(address_length).map_err(|_| {
            LocalApiError::InvalidEndpoint("Unix socket address is too long".to_string())
        })?;
    }
    if address_length > size_of::<libc::sockaddr_un>() {
        return Err(LocalApiError::InvalidEndpoint(
            "Unix socket address is too long".to_string(),
        ));
    }

    // SAFETY: address points to an initialized sockaddr_un for address_length
    // bytes and raw_fd remains owned for the duration of the call.
    let result = unsafe {
        libc::connect(
            raw_fd,
            (&raw const address).cast::<libc::sockaddr>(),
            address_length as libc::socklen_t,
        )
    };
    if result < 0 {
        let error = io::Error::last_os_error();
        if !matches!(error.raw_os_error(), Some(code)
            if code == libc::EINPROGRESS
                || code == libc::EAGAIN
                || code == libc::EWOULDBLOCK)
        {
            return Err(LocalApiError::Io(error));
        }
        wait_unix_connected(raw_fd, deadline)?;
    }

    // SAFETY: owned uniquely owns raw_fd. Converting it transfers that
    // ownership into UnixStream exactly once.
    let stream = std::os::unix::net::UnixStream::from(owned);
    stream.set_nonblocking(false)?;
    Ok(stream)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn wait_unix_connected(raw_fd: std::os::fd::RawFd, deadline: Instant) -> Result<(), LocalApiError> {
    loop {
        let timeout = poll_timeout_millis(remaining(deadline)?)?;
        let mut descriptor = libc::pollfd {
            fd: raw_fd,
            events: libc::POLLOUT,
            revents: 0,
        };
        // SAFETY: descriptor is a valid one-element pollfd array.
        let result = unsafe { libc::poll(&mut descriptor, 1, timeout) };
        if result == 0 {
            return Err(LocalApiError::Timeout);
        }
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(LocalApiError::Io(error));
        }
        let mut socket_error = 0_i32;
        let mut length = std::mem::size_of::<i32>() as libc::socklen_t;
        // SAFETY: both output pointers are valid for their declared sizes.
        let result = unsafe {
            libc::getsockopt(
                raw_fd,
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                (&raw mut socket_error).cast(),
                &mut length,
            )
        };
        if result < 0 {
            return Err(LocalApiError::Io(io::Error::last_os_error()));
        }
        if socket_error != 0 {
            return Err(LocalApiError::Io(io::Error::from_raw_os_error(
                socket_error,
            )));
        }
        return Ok(());
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn poll_timeout_millis(duration: Duration) -> Result<i32, LocalApiError> {
    if duration.is_zero() {
        return Err(LocalApiError::Timeout);
    }
    let millis = duration.as_nanos().div_ceil(1_000_000);
    Ok(millis.min(i32::MAX as u128) as i32)
}

#[cfg(windows)]
mod windows_pipe {
    use super::{LocalApiError, remaining};
    use std::ffi::{OsStr, c_void};
    use std::io;
    use std::mem::zeroed;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};
    use std::time::{Duration, Instant};

    type Handle = *mut c_void;

    const INVALID_HANDLE_VALUE: Handle = (-1_isize) as Handle;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_OVERLAPPED: u32 = 0x4000_0000;
    const SECURITY_SQOS_PRESENT: u32 = 0x0010_0000;
    const SECURITY_IDENTIFICATION: u32 = 0x0001_0000;
    const ERROR_FILE_NOT_FOUND: i32 = 2;
    const ERROR_BROKEN_PIPE: i32 = 109;
    const ERROR_SEM_TIMEOUT: i32 = 121;
    const ERROR_PIPE_BUSY: i32 = 231;
    const ERROR_PIPE_NOT_CONNECTED: i32 = 233;
    const ERROR_IO_PENDING: i32 = 997;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_TIMEOUT: u32 = 258;
    const WAIT_FAILED: u32 = u32::MAX;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct OverlappedOffset {
        offset: u32,
        offset_high: u32,
    }

    #[repr(C)]
    union OverlappedPosition {
        offsets: OverlappedOffset,
        pointer: *mut c_void,
    }

    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        position: OverlappedPosition,
        event: Handle,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateFileW(
            name: *const u16,
            desired_access: u32,
            share_mode: u32,
            security_attributes: *const c_void,
            creation_disposition: u32,
            flags_and_attributes: u32,
            template_file: Handle,
        ) -> Handle;
        fn WaitNamedPipeW(name: *const u16, timeout_millis: u32) -> i32;
        fn CreateEventW(
            event_attributes: *const c_void,
            manual_reset: i32,
            initial_state: i32,
            name: *const u16,
        ) -> Handle;
        fn ReadFile(
            file: Handle,
            buffer: *mut c_void,
            bytes_to_read: u32,
            bytes_read: *mut u32,
            overlapped: *mut Overlapped,
        ) -> i32;
        fn WriteFile(
            file: Handle,
            buffer: *const c_void,
            bytes_to_write: u32,
            bytes_written: *mut u32,
            overlapped: *mut Overlapped,
        ) -> i32;
        fn GetOverlappedResult(
            file: Handle,
            overlapped: *mut Overlapped,
            transferred: *mut u32,
            wait: i32,
        ) -> i32;
        fn CancelIoEx(file: Handle, overlapped: *mut Overlapped) -> i32;
        fn WaitForSingleObject(handle: Handle, timeout_millis: u32) -> u32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    pub(super) struct NamedPipe {
        handle: Handle,
    }

    #[derive(Clone, Copy)]
    enum IoOperation {
        Read,
        Write,
    }

    struct PendingIo {
        overlapped: Overlapped,
        buffer: Vec<u8>,
        event: Handle,
    }

    impl NamedPipe {
        pub(super) fn connect(path: &OsStr, deadline: Instant) -> Result<Self, LocalApiError> {
            let mut wide: Vec<u16> = path.encode_wide().collect();
            if wide.is_empty() || wide.contains(&0) {
                return Err(LocalApiError::InvalidEndpoint(
                    "named-pipe path is empty or contains NUL".to_string(),
                ));
            }
            wide.push(0);
            loop {
                let _ = remaining(deadline)?;
                // SAFETY: wide is NUL-terminated and all other pointer
                // arguments are null as permitted by CreateFileW.
                let handle = unsafe {
                    CreateFileW(
                        wide.as_ptr(),
                        GENERIC_READ | GENERIC_WRITE,
                        0,
                        null(),
                        OPEN_EXISTING,
                        FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                        null_mut(),
                    )
                };
                if handle != INVALID_HANDLE_VALUE {
                    return Ok(Self { handle });
                }
                let error = io::Error::last_os_error();
                match error.raw_os_error() {
                    Some(ERROR_PIPE_BUSY) => {
                        let wait = wait_slice_millis(remaining(deadline)?);
                        // SAFETY: wide remains NUL-terminated for the call.
                        let result = unsafe { WaitNamedPipeW(wide.as_ptr(), wait) };
                        if result == 0 {
                            let wait_error = io::Error::last_os_error();
                            if Instant::now() >= deadline {
                                return Err(LocalApiError::Timeout);
                            }
                            if !wait_named_pipe_error_is_retryable(wait_error.raw_os_error()) {
                                return Err(LocalApiError::Io(wait_error));
                            }
                        }
                    }
                    Some(ERROR_FILE_NOT_FOUND) => {
                        let pause = remaining(deadline)?.min(Duration::from_millis(10));
                        std::thread::sleep(pause);
                    }
                    _ => return Err(LocalApiError::Io(error)),
                }
            }
        }

        pub(super) fn read_deadline(
            &mut self,
            buffer: &mut [u8],
            deadline: Instant,
        ) -> Result<usize, LocalApiError> {
            let (count, owned) =
                self.overlapped_io(IoOperation::Read, vec![0; buffer.len()], deadline)?;
            buffer[..count].copy_from_slice(&owned[..count]);
            Ok(count)
        }

        pub(super) fn write_deadline(
            &mut self,
            buffer: &[u8],
            deadline: Instant,
        ) -> Result<usize, LocalApiError> {
            let (count, _) = self.overlapped_io(IoOperation::Write, buffer.to_vec(), deadline)?;
            Ok(count)
        }

        fn overlapped_io(
            &mut self,
            operation: IoOperation,
            buffer: Vec<u8>,
            deadline: Instant,
        ) -> Result<(usize, Vec<u8>), LocalApiError> {
            if buffer.is_empty() {
                return Ok((0, buffer));
            }
            if self.handle == INVALID_HANDLE_VALUE {
                return Err(LocalApiError::ConnectionNotReusable);
            }
            let requested = u32::try_from(buffer.len()).map_err(|_| LocalApiError::BodyTooLarge)?;
            // SAFETY: null security attributes and name create a private manual
            // reset event owned by this operation.
            let event = unsafe { CreateEventW(null(), 1, 0, null()) };
            if event.is_null() {
                return Err(LocalApiError::Io(io::Error::last_os_error()));
            }
            // SAFETY: OVERLAPPED is a C POD structure for which zero is the
            // documented initial state.
            let mut pending = Box::new(PendingIo {
                overlapped: unsafe { zeroed() },
                buffer,
                event,
            });
            pending.overlapped.event = event;
            // The OVERLAPPED and I/O bytes are heap-owned at stable addresses.
            // If cancellation does not complete by the absolute deadline, the
            // pipe is poisoned and this allocation is intentionally retained
            // for process lifetime so the kernel can never access freed memory.
            let started = match operation {
                // SAFETY: the heap buffer and OVERLAPPED remain stable until
                // completion or are intentionally leaked after handle close.
                IoOperation::Read => unsafe {
                    ReadFile(
                        self.handle,
                        pending.buffer.as_mut_ptr().cast(),
                        requested,
                        null_mut(),
                        &mut pending.overlapped,
                    )
                },
                // SAFETY: same ownership guarantee as the read case.
                IoOperation::Write => unsafe {
                    WriteFile(
                        self.handle,
                        pending.buffer.as_ptr().cast(),
                        requested,
                        null_mut(),
                        &mut pending.overlapped,
                    )
                },
            };
            if started != 0 {
                return self.finish_io(pending);
            }
            let start_error = io::Error::last_os_error();
            if start_error.raw_os_error() != Some(ERROR_IO_PENDING) {
                if matches!(start_error.raw_os_error(), Some(code)
                    if code == ERROR_BROKEN_PIPE || code == ERROR_PIPE_NOT_CONNECTED)
                {
                    return Ok((0, std::mem::take(&mut pending.buffer)));
                }
                return Err(LocalApiError::Io(start_error));
            }

            let wait = match remaining(deadline) {
                Ok(remaining) => wait_millis(remaining),
                Err(error) => {
                    self.cancel_or_abandon(pending);
                    return Err(error);
                }
            };
            // SAFETY: event remains owned and valid for this wait.
            match unsafe { WaitForSingleObject(event, wait) } {
                WAIT_OBJECT_0 => self.finish_io(pending),
                WAIT_TIMEOUT => {
                    self.cancel_or_abandon(pending);
                    Err(LocalApiError::Timeout)
                }
                WAIT_FAILED => {
                    let error = io::Error::last_os_error();
                    self.cancel_or_abandon(pending);
                    Err(LocalApiError::Io(error))
                }
                _ => {
                    self.cancel_or_abandon(pending);
                    Err(LocalApiError::Protocol("unexpected Windows wait result"))
                }
            }
        }

        // The box is load-bearing: Windows retains the OVERLAPPED address from
        // the start call until completion or the bounded abandon path.
        #[allow(clippy::boxed_local)]
        fn finish_io(
            &self,
            mut pending: Box<PendingIo>,
        ) -> Result<(usize, Vec<u8>), LocalApiError> {
            let mut transferred = 0_u32;
            // SAFETY: the operation either completed synchronously or its event
            // signaled, so this query cannot outlive the heap-owned state.
            if unsafe {
                GetOverlappedResult(self.handle, &mut pending.overlapped, &mut transferred, 0)
            } == 0
            {
                let error = io::Error::last_os_error();
                if matches!(error.raw_os_error(), Some(code)
                    if code == ERROR_BROKEN_PIPE || code == ERROR_PIPE_NOT_CONNECTED)
                {
                    return Ok((0, std::mem::take(&mut pending.buffer)));
                }
                return Err(LocalApiError::Io(error));
            }
            let transferred = transferred as usize;
            if transferred > pending.buffer.len() {
                return Err(LocalApiError::Protocol(
                    "Windows named-pipe transfer exceeded its buffer",
                ));
            }
            Ok((transferred, std::mem::take(&mut pending.buffer)))
        }

        fn cancel_or_abandon(&mut self, mut pending: Box<PendingIo>) {
            // SAFETY: the pending OVERLAPPED belongs to this uniquely owned
            // handle. Cancellation is best-effort; completion is polled with a
            // zero timeout to preserve the single absolute request deadline.
            unsafe {
                CancelIoEx(self.handle, &mut pending.overlapped);
            }
            // SAFETY: the private event remains valid in pending.
            let completed = unsafe { WaitForSingleObject(pending.event, 0) } == WAIT_OBJECT_0;
            if completed {
                let mut transferred = 0_u32;
                // SAFETY: a signaled per-operation event means the kernel has
                // finished using OVERLAPPED and its buffer. The result itself
                // may be ERROR_OPERATION_ABORTED and is intentionally ignored.
                unsafe {
                    GetOverlappedResult(self.handle, &mut pending.overlapped, &mut transferred, 0);
                }
            }

            // A timed-out HTTP stream is never reusable, even when Windows
            // reports cancellation synchronously. Close and poison it in both
            // cases. If completion is not observable, retain the heap state
            // forever so a late kernel completion cannot touch freed memory.
            let handle = std::mem::replace(&mut self.handle, INVALID_HANDLE_VALUE);
            if handle != INVALID_HANDLE_VALUE {
                // SAFETY: ownership was removed above and is closed once.
                unsafe {
                    CloseHandle(handle);
                }
            }
            if !completed {
                let _ = Box::leak(pending);
            }
        }
    }

    impl Drop for NamedPipe {
        fn drop(&mut self) {
            if self.handle != INVALID_HANDLE_VALUE {
                // SAFETY: handle is uniquely owned and closed once here.
                unsafe {
                    CloseHandle(self.handle);
                }
            }
        }
    }

    impl Drop for PendingIo {
        fn drop(&mut self) {
            // SAFETY: a non-leaked PendingIo owns this event and is dropped only
            // after completion or an immediate start failure.
            unsafe {
                CloseHandle(self.event);
            }
        }
    }

    fn wait_slice_millis(duration: Duration) -> u32 {
        wait_millis(duration.min(Duration::from_millis(25)))
    }

    pub(super) fn wait_named_pipe_error_is_retryable(error: Option<i32>) -> bool {
        matches!(error, Some(code)
            if code == ERROR_PIPE_BUSY
                || code == ERROR_FILE_NOT_FOUND
                || code == ERROR_SEM_TIMEOUT)
    }

    fn wait_millis(duration: Duration) -> u32 {
        let rounded = duration.as_nanos().div_ceil(1_000_000);
        rounded.clamp(1, u128::from(u32::MAX - 1)) as u32
    }
}

#[cfg(test)]
struct TestStream {
    input: std::collections::VecDeque<u8>,
    output: Vec<u8>,
    max_read: usize,
}

#[cfg(test)]
impl TestStream {
    fn new(input: impl Into<Vec<u8>>, max_read: usize) -> Self {
        Self {
            input: input.into().into(),
            output: Vec::new(),
            max_read: max_read.max(1),
        }
    }
}

#[cfg(test)]
impl Read for TestStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = buffer.len().min(self.max_read).min(self.input.len());
        for destination in &mut buffer[..count] {
            *destination = self.input.pop_front().expect("count was bounded");
        }
        Ok(count)
    }
}

#[cfg(test)]
impl Write for TestStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.output.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::thread;

    const ETAG: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const NULL_ETAG: &str = "74234e98afe7498fb5daf1f36ac2d78acc339464f950703b8c019892f982b90b";

    fn exact_response(status: &str, extra_headers: &str, body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 {status}\r\nTailscale-Cap: 142\r\nTailscale-Version: 1.102.2\r\n{extra_headers}Content-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    fn memory_connection(input: &[u8], max_read: usize) -> Connection {
        Connection {
            stream: TransportStream::Memory(TestStream::new(input, max_read)),
            buffered: ReadBuffer::default(),
            reusable: true,
            compatibility: DaemonCompatibility::ExactV1_102_2,
        }
    }

    #[test]
    fn exact_request_headers_and_cas_etag() {
        let response = exact_response("200 OK", "", b"");
        let mut connection = memory_connection(&response, 3);
        let result = connection.request(
            "POST",
            "/localapi/v0/serve-config",
            &[("If-Match", ETAG), ("Content-Type", "application/json")],
            b"{}",
            absolute_deadline(),
        );
        assert!(result.is_ok(), "{result:?}");
        let TransportStream::Memory(stream) = connection.stream else {
            panic!("expected memory transport")
        };
        let request = String::from_utf8(stream.output).expect("ASCII request");
        assert!(request.starts_with(
            "POST /localapi/v0/serve-config HTTP/1.1\r\nHost: local-tailscaled.sock\r\n"
        ));
        assert!(request.contains("Tailscale-Cap: 142\r\n"));
        assert!(request.contains(&format!("If-Match: {ETAG}\r\n")));
        assert!(request.contains("Content-Length: 2\r\n\r\n{}"));
    }

    #[test]
    fn chunked_response_is_bounded_and_preserves_next_response() {
        let input = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nnull\r\n0\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";
        let mut stream = TransportStream::Memory(TestStream::new(input, 2));
        let mut buffered = ReadBuffer::default();
        let first =
            read_response(&mut stream, &mut buffered, absolute_deadline()).expect("first response");
        assert_eq!(first.body, b"null");
        let second = read_response(&mut stream, &mut buffered, absolute_deadline())
            .expect("second response");
        assert_eq!(second.body, b"{}");
    }

    #[test]
    fn conflicting_framing_is_rejected() {
        let input = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nTransfer-Encoding: chunked\r\n\r\n{}";
        let mut stream = TransportStream::Memory(TestStream::new(input, 64));
        let error = read_response(&mut stream, &mut ReadBuffer::default(), absolute_deadline())
            .expect_err("conflicting framing must fail");
        assert!(matches!(error, LocalApiError::Protocol(_)));
    }

    #[test]
    fn chunk_extensions_and_trailers_are_rejected() {
        for input in [
            &b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1;x=y\r\na\r\n0\r\n\r\n"[..],
            &b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\na\r\n0\r\nX: y\r\n\r\n"[..],
        ] {
            let mut stream = TransportStream::Memory(TestStream::new(input, 64));
            let error = read_response(&mut stream, &mut ReadBuffer::default(), absolute_deadline())
                .expect_err("unsupported chunk syntax must fail");
            assert!(matches!(error, LocalApiError::Protocol(_)));
        }
    }

    #[test]
    fn status_binding_uses_stable_id_and_https_capability() {
        let status = parse_status(
            br#"{"BackendState":"Running","CertDomains":["example-host.tailnet-example.ts.net"],"Self":{"DNSName":"example-host.tailnet-example.ts.net.","ID":"n-stable","NodeID":123,"CapMap":{"https":null}}}"#
                .to_vec(),
        )
        .expect("valid status");
        assert_eq!(status.backend_state(), "Running");
        assert_eq!(
            status.self_dns_name(),
            "example-host.tailnet-example.ts.net."
        );
        assert_eq!(status.self_stable_id(), "n-stable");
        assert_eq!(status.self_node_id(), Some("123"));
        assert!(status.https_capable());
        assert_eq!(
            status.cert_domains(),
            &["example-host.tailnet-example.ts.net"]
        );
        assert!(status.has_cert_domain("example-host.tailnet-example.ts.net"));
        assert!(!status.has_cert_domain("example-host.tailnet-example.ts.net."));
    }

    #[test]
    fn status_cert_domain_uniqueness_is_linear_and_bounded() {
        let domains = (0..4_000)
            .map(|index| Value::String(format!("host-{index}.tailnet-example.ts.net")))
            .collect::<Vec<_>>();
        let parsed = parse_cert_domains(Some(&Value::Array(domains))).expect("unique domains");
        assert_eq!(parsed.len(), 4_000);

        let duplicate = Value::Array(vec![
            Value::String("same.tailnet-example.ts.net".to_string()),
            Value::String("same.tailnet-example.ts.net".to_string()),
        ]);
        assert!(parse_cert_domains(Some(&duplicate)).is_err());
    }

    #[test]
    fn etag_is_exact_unquoted_lower_hex() {
        assert!(validate_etag(ETAG).is_ok());
        assert!(validate_etag(&ETAG.to_ascii_uppercase()).is_err());
        assert!(validate_etag(&format!("\"{ETAG}\"")).is_err());
        assert!(validate_etag("").is_err());
        assert!(validate_serve_config_etag(NULL_ETAG, b"null").is_ok());
        assert!(validate_serve_config_etag(ETAG, b"null").is_err());
        assert!(validate_serve_config_etag(NULL_ETAG, b"{}").is_err());
    }

    #[test]
    fn duplicate_json_keys_are_rejected_recursively() {
        for input in [
            &br#"{"same":1,"same":2}"#[..],
            &br#"{"outer":{"same":1,"same":2}}"#[..],
            &br#"[{"same":1,"same":2}]"#[..],
        ] {
            let error = parse_json_no_duplicates(input).expect_err("duplicate key must fail");
            assert!(matches!(error, LocalApiError::InvalidJson(_)));
        }
        assert!(parse_json_no_duplicates(br#"{"one":1,"nested":{"two":2}}"#).is_ok());
    }

    #[test]
    fn daemon_identity_headers_are_unique_and_exact_for_apply() {
        let invalid = [
            "Tailscale-Cap: 141\r\nTailscale-Version: 1.102.2\r\n",
            "Tailscale-Cap: 142\r\nTailscale-Version: 1.102.1\r\n",
            "Tailscale-Cap: 142\r\nTailscale-Version: 1.102.2-\r\n",
            "Tailscale-Cap: 142\r\nTailscale-Cap: 142\r\nTailscale-Version: 1.102.2\r\n",
            "Tailscale-Cap: 142\r\nTailscale-Version: 1.102.2\r\nTailscale-Version: 1.102.2\r\n",
        ];
        for headers in invalid {
            let response = format!("HTTP/1.1 200 OK\r\n{headers}Content-Length: 0\r\n\r\n");
            let mut connection = memory_connection(response.as_bytes(), 64);
            let error = connection
                .request(
                    "GET",
                    "/localapi/v0/status?peers=false",
                    &[],
                    &[],
                    absolute_deadline(),
                )
                .expect_err("invalid daemon identity must fail");
            assert!(matches!(
                error,
                LocalApiError::IncompatibleDaemon(_) | LocalApiError::Protocol(_)
            ));
        }

        let later = HttpResponse {
            status: 200,
            headers: vec![
                ("tailscale-cap".into(), "143".into()),
                ("tailscale-version".into(), "1.103.0-t1+build".into()),
            ],
            body: Vec::new(),
            connection_close: false,
        };
        assert!(validate_daemon_headers(&later, DaemonCompatibility::ExactV1_102_2).is_err());
        assert!(validate_daemon_headers(&later, DaemonCompatibility::CleanupVersionDrift).is_ok());

        for (capability, version) in [("141", "1.102.2"), ("142", "1.102.1")] {
            let downgrade = HttpResponse {
                status: 200,
                headers: vec![
                    ("tailscale-cap".into(), capability.into()),
                    ("tailscale-version".into(), version.into()),
                ],
                body: Vec::new(),
                connection_close: false,
            };
            assert!(
                validate_daemon_headers(&downgrade, DaemonCompatibility::CleanupVersionDrift)
                    .is_err()
            );
        }

        let next_major = HttpResponse {
            status: 200,
            headers: vec![
                ("tailscale-cap".into(), "200".into()),
                ("tailscale-version".into(), "2.0.0".into()),
            ],
            body: Vec::new(),
            connection_close: false,
        };
        assert!(
            validate_daemon_headers(&next_major, DaemonCompatibility::CleanupVersionDrift).is_err(),
            "cleanup drift is bounded to Tailscale major version 1"
        );
    }

    #[test]
    fn response_header_and_body_caps_are_enforced() {
        let mut oversized_headers = b"HTTP/1.1 200 OK\r\nX-Fill: ".to_vec();
        oversized_headers.extend(std::iter::repeat_n(b'a', HEADER_LIMIT));
        oversized_headers.extend_from_slice(b"\r\n\r\n");
        let mut stream =
            TransportStream::Memory(TestStream::new(oversized_headers, IO_BUFFER_SIZE));
        assert!(matches!(
            read_response(&mut stream, &mut ReadBuffer::default(), absolute_deadline()),
            Err(LocalApiError::HeaderTooLarge)
        ));

        let oversized_body = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
            BODY_LIMIT + 1
        );
        let mut stream = TransportStream::Memory(TestStream::new(oversized_body, IO_BUFFER_SIZE));
        assert!(matches!(
            read_response(&mut stream, &mut ReadBuffer::default(), absolute_deadline()),
            Err(LocalApiError::BodyTooLarge)
        ));
    }

    #[test]
    fn close_delimited_response_succeeds_once_then_connection_is_not_reused() {
        let response = b"HTTP/1.1 200 OK\r\nTailscale-Cap: 142\r\nTailscale-Version: 1.102.2\r\nConnection: close\r\n\r\nnull";
        let mut connection = memory_connection(response, 5);
        let first = connection
            .request(
                "GET",
                "/localapi/v0/serve-config",
                &[],
                &[],
                absolute_deadline(),
            )
            .expect("current close-delimited response remains usable");
        assert_eq!(first.body, b"null");
        assert!(matches!(
            connection.request(
                "GET",
                "/localapi/v0/status?peers=false",
                &[],
                &[],
                absolute_deadline()
            ),
            Err(LocalApiError::ConnectionNotReusable)
        ));
    }

    #[test]
    fn serve_cas_412_is_typed_no_mutation() {
        let response = exact_response("412 Precondition Failed", "", b"");
        let mut session = LocalApiSession {
            connection: memory_connection(&response, 7),
            deadline: absolute_deadline(),
        };
        let error = session
            .compare_and_set_serve_config(ETAG, b"{}")
            .expect_err("stale ETag must fail");
        assert!(matches!(error, ServeConfigCasError::PreconditionFailed));
    }

    #[test]
    fn serve_cas_access_denial_after_post_is_indeterminate() {
        for (status_line, expected_status) in [("401 Unauthorized", 401), ("403 Forbidden", 403)] {
            let response = exact_response(status_line, "", b"");
            let mut session = LocalApiSession {
                connection: memory_connection(&response, 7),
                deadline: absolute_deadline(),
            };
            let error = session
                .compare_and_set_serve_config(ETAG, b"{}")
                .expect_err("an access-denied POST response cannot prove no mutation");
            assert!(matches!(
                error,
                ServeConfigCasError::Indeterminate(LocalApiError::AccessDenied(status))
                    if status == expected_status
            ));
        }
    }

    #[test]
    fn test_tcp_override_is_loopback_only() {
        assert!(TailscaleLocalApiClient::with_test_tcp("127.0.0.1:1".parse().unwrap()).is_ok());
        assert!(TailscaleLocalApiClient::with_test_tcp("[::1]:1".parse().unwrap()).is_ok());
        assert!(TailscaleLocalApiClient::with_test_tcp("192.0.2.1:1".parse().unwrap()).is_err());
        assert!(TailscaleLocalApiClient::with_test_tcp("127.0.0.1:0".parse().unwrap()).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn named_pipe_wait_timeout_is_retryable() {
        assert!(windows_pipe::wait_named_pipe_error_is_retryable(Some(121)));
        assert!(windows_pipe::wait_named_pipe_error_is_retryable(Some(231)));
        assert!(windows_pipe::wait_named_pipe_error_is_retryable(Some(2)));
        assert!(!windows_pipe::wait_named_pipe_error_is_retryable(Some(5)));
    }

    #[cfg(windows)]
    #[test]
    fn named_pipe_pending_read_timeout_is_bounded_and_poisoned() {
        use std::ffi::{OsStr, c_void};
        use std::os::windows::ffi::OsStrExt;
        use std::ptr::null;

        type Handle = *mut c_void;
        const INVALID_HANDLE_VALUE: Handle = (-1_isize) as Handle;
        const PIPE_ACCESS_DUPLEX: u32 = 0x0000_0003;
        const ERROR_PIPE_CONNECTED: i32 = 535;

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CreateNamedPipeW(
                name: *const u16,
                open_mode: u32,
                pipe_mode: u32,
                max_instances: u32,
                out_buffer_size: u32,
                in_buffer_size: u32,
                default_timeout: u32,
                security_attributes: *const c_void,
            ) -> Handle;
            fn ConnectNamedPipe(pipe: Handle, overlapped: *mut c_void) -> i32;
            fn CloseHandle(handle: Handle) -> i32;
        }

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let path = format!(
            r"\\.\pipe\ferric-localapi-timeout-{}-{unique}",
            std::process::id()
        );
        let mut wide = OsStr::new(&path).encode_wide().collect::<Vec<_>>();
        wide.push(0);
        // SAFETY: the path is NUL-terminated and remaining arguments are the
        // documented byte-mode, blocking named-pipe server defaults.
        let server = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                0,
                1,
                4_096,
                4_096,
                0,
                null(),
            )
        };
        assert_ne!(server, INVALID_HANDLE_VALUE, "create test named pipe");
        let server_bits = server as usize;
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
        let server_thread = thread::spawn(move || {
            let server = server_bits as Handle;
            // SAFETY: this thread uniquely owns the server handle. A client
            // may connect just before the call, which is the documented 535
            // success-equivalent race.
            let connected = unsafe { ConnectNamedPipe(server, std::ptr::null_mut()) };
            if connected == 0 {
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(ERROR_PIPE_CONNECTED)
                );
            }
            ready_tx.send(()).expect("signal connected server");
            done_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("client completed bounded timeout assertions");
            // SAFETY: the server handle is closed exactly once here.
            unsafe { CloseHandle(server) };
        });

        let mut pipe = windows_pipe::NamedPipe::connect(
            OsStr::new(&path),
            Instant::now() + Duration::from_secs(1),
        )
        .expect("connect test named pipe");
        ready_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("server completed named-pipe connection");
        let started = Instant::now();
        let mut byte = [0_u8; 1];
        assert!(matches!(
            pipe.read_deadline(&mut byte, Instant::now() + Duration::from_millis(25)),
            Err(LocalApiError::Timeout)
        ));
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "pending named-pipe read exceeded its bound"
        );
        assert!(matches!(
            pipe.read_deadline(&mut byte, Instant::now() + Duration::from_millis(25)),
            Err(LocalApiError::ConnectionNotReusable)
        ));
        done_tx
            .send(())
            .expect("release named-pipe test server after assertions");
        server_thread.join().expect("named-pipe server survived");
    }

    #[test]
    fn stalled_partial_response_honors_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let address = listener.local_addr().expect("listener address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("connection");
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n")
                .expect("partial response");
            thread::sleep(Duration::from_millis(100));
        });
        let stream = TcpStream::connect(address).expect("connect loopback");
        let mut stream = TransportStream::Tcp(stream);
        let error = read_response(
            &mut stream,
            &mut ReadBuffer::default(),
            Instant::now() + Duration::from_millis(20),
        )
        .expect_err("partial response must time out");
        assert!(matches!(error, LocalApiError::Timeout));
        server.join().expect("server joined");
    }

    #[test]
    fn post_send_timeout_is_indeterminate() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let address = listener.local_addr().expect("listener address");
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("connection");
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .expect("read timeout");
            let mut reader = BufReader::new(stream);
            let request = read_test_request(&mut reader);
            assert!(request.starts_with("POST /localapi/v0/serve-config HTTP/1.1\r\n"));
            thread::sleep(Duration::from_millis(100));
        });
        let client = TailscaleLocalApiClient::with_test_tcp(address).expect("test client");
        let mut session = client.open_session().expect("open session");
        session.deadline = Instant::now() + Duration::from_millis(20);
        let error = session
            .compare_and_set_serve_config(ETAG, b"{}")
            .expect_err("missing POST response must be ambiguous");
        assert!(matches!(
            error,
            ServeConfigCasError::Indeterminate(LocalApiError::Timeout)
        ));
        server.join().expect("server joined");
    }

    #[test]
    fn session_reuses_one_connection_for_status_serve_status() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let address = listener.local_addr().expect("listener address");
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("one connection");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("read timeout");
            let mut reader = BufReader::new(stream);
            let first = read_test_request(&mut reader);
            assert!(first.starts_with("GET /localapi/v0/status?peers=false HTTP/1.1\r\n"));
            let status = br#"{"BackendState":"Running","CertDomains":["example-host.tailnet-example.ts.net"],"Self":{"DNSName":"example-host.tailnet-example.ts.net.","ID":"n-stable","NodeID":123,"CapMap":{"https":[]}}}"#;
            write!(
                reader.get_mut(),
                "HTTP/1.1 200 OK\r\nTailscale-Cap: 142\r\nTailscale-Version: 1.102.2\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                status.len()
            )
            .expect("status headers");
            reader.get_mut().write_all(status).expect("status body");

            let second = read_test_request(&mut reader);
            assert!(second.starts_with("GET /localapi/v0/serve-config HTTP/1.1\r\n"));
            write!(
                reader.get_mut(),
                "HTTP/1.1 200 OK\r\nTailscale-Cap: 142\r\nTailscale-Version: 1.102.2\r\nContent-Type: application/json\r\nEtag: {NULL_ETAG}\r\nContent-Length: 4\r\n\r\nnull"
            )
            .expect("serve response");

            let third = read_test_request(&mut reader);
            assert!(third.starts_with("GET /localapi/v0/status?peers=false HTTP/1.1\r\n"));
            write!(
                reader.get_mut(),
                "HTTP/1.1 200 OK\r\nTailscale-Cap: 142\r\nTailscale-Version: 1.102.2\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                status.len()
            )
            .expect("second status headers");
            reader
                .get_mut()
                .write_all(status)
                .expect("second status body");
        });

        let client = TailscaleLocalApiClient::with_test_tcp(address).expect("test client");
        let mut session = client.open_session().expect("same-connection session");
        let before = session.get_status().expect("status before Serve read");
        let serve = session.get_serve_config().expect("Serve read");
        let after = session.get_status().expect("status after Serve read");
        assert_eq!(before.self_stable_id(), "n-stable");
        assert_eq!(serve.etag(), NULL_ETAG);
        assert_eq!(after, before);
        server.join().expect("server joined");
    }

    fn read_test_request(reader: &mut BufReader<TcpStream>) -> String {
        let mut request = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("request line");
            assert!(!line.is_empty(), "unexpected request EOF");
            request.push_str(&line);
            if line == "\r\n" {
                return request;
            }
        }
    }
}

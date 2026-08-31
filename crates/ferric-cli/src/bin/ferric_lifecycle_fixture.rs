//! Model-free `llama-server` lifecycle fixture.
//!
//! This binary exists only behind the `lifecycle-fixture` feature. Integration
//! tests copy it to the platform's exact `llama-server` filename so Ferric's
//! real closed-engine launch, identity inspection, and teardown paths run
//! without loading a model.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

const INVOCATION_MARKER_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_INVOCATION_MARKER";
const LIFETIME_TOKEN_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_LIFETIME_TOKEN";
const BIND_DIAGNOSTIC_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_BIND_DIAGNOSTIC";
const READY_MARKER_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_READY_MARKER";
const TAILSCALE_STATE_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_TAILSCALE_STATE";
const TAILSCALE_LOG_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_TAILSCALE_LOG";
const TAILSCALE_LOCAL_JOURNAL_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_TAILSCALE_LOCAL_JOURNAL";
const TAILSCALE_GLOBAL_JOURNAL_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_TAILSCALE_GLOBAL_JOURNAL";
const TAILSCALE_FQDN: &str = "example-host.tailnet-example.ts.net";
const TAILSCALE_HTTPS_PORT: u16 = 443;
const ADDRESS_IN_USE_DIAGNOSTIC: &[u8] = b"ferric-lifecycle-fixture:address-in-use:v1\n";
const READY_MARKER: &[u8] = b"ferric-lifecycle-fixture:ready:v1\n";

#[derive(Debug)]
enum TailscaleRequest {
    WhoAmI,
    Status,
    Apply { mount_path: String, target: String },
    Off { mount_path: String },
}

fn invoked_as_tailscale() -> std::io::Result<bool> {
    let executable = std::env::current_exe()?;
    let Some(filename) = executable.file_name().and_then(|name| name.to_str()) else {
        return Ok(false);
    };
    Ok(matches!(
        filename.to_ascii_lowercase().as_str(),
        "tailscale" | "tailscale.exe"
    ))
}

fn parse_mount_path(value: &str) -> Result<String, String> {
    let token = value
        .strip_prefix("/_ferric/")
        .ok_or_else(|| "fixture Tailscale mutation requires /_ferric/<token>".to_string())?;
    if token.len() != 32
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("fixture Tailscale token must be 32 lowercase hexadecimal digits".to_string());
    }
    Ok(value.to_string())
}

fn proxy_target_port(value: &str) -> Result<u16, String> {
    let raw_port = value
        .strip_prefix("http://127.0.0.1:")
        .ok_or_else(|| "fixture Tailscale target must be exact loopback HTTP".to_string())?;
    let port = raw_port
        .parse::<u16>()
        .map_err(|_| "fixture Tailscale target port must be a nonzero u16".to_string())?;
    if port == 0 || raw_port != port.to_string() {
        return Err("fixture Tailscale target port must be canonical and nonzero".to_string());
    }
    Ok(port)
}

fn parse_tailscale_request(arguments: &[String]) -> Result<TailscaleRequest, String> {
    match arguments {
        [command, json] if command == "whoami" && json == "--json" => Ok(TailscaleRequest::WhoAmI),
        [serve, status, json] if serve == "serve" && status == "status" && json == "--json" => {
            Ok(TailscaleRequest::Status)
        }
        [serve, background, https, set_path, yes, value]
            if serve == "serve"
                && background == "--bg"
                && https == "--https=443"
                && yes == "--yes" =>
        {
            let mount_path = set_path
                .strip_prefix("--set-path=")
                .ok_or_else(|| "fixture Tailscale mutation requires --set-path".to_string())?;
            let mount_path = parse_mount_path(mount_path)?;
            if value == "off" {
                Ok(TailscaleRequest::Off { mount_path })
            } else {
                proxy_target_port(value)?;
                Ok(TailscaleRequest::Apply {
                    mount_path,
                    target: value.clone(),
                })
            }
        }
        _ => Err(format!(
            "fixture rejects unsupported Tailscale argv: {arguments:?}"
        )),
    }
}

fn tailscale_state_path() -> Result<PathBuf, String> {
    std::env::var_os(TAILSCALE_STATE_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| format!("fixture requires {TAILSCALE_STATE_ENV}"))
}

fn read_tailscale_state(path: &Path) -> Result<serde_json::Value, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("could not read fixture Tailscale state: {error}"))?;
    let state: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("fixture Tailscale state is malformed: {error}"))?;
    if !state.is_object() {
        return Err("fixture Tailscale state root must be an object".to_string());
    }
    Ok(state)
}

fn write_tailscale_state(path: &Path, state: &serde_json::Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(state)
        .map_err(|error| format!("could not serialize fixture Tailscale state: {error}"))?;
    bytes.push(b'\n');
    std::fs::write(path, bytes)
        .map_err(|error| format!("could not write fixture Tailscale state: {error}"))
}

fn journal_paths() -> Result<Option<(PathBuf, PathBuf)>, String> {
    match (
        std::env::var_os(TAILSCALE_LOCAL_JOURNAL_ENV),
        std::env::var_os(TAILSCALE_GLOBAL_JOURNAL_ENV),
    ) {
        (None, None) => Ok(None),
        (Some(local), Some(global)) => Ok(Some((local.into(), global.into()))),
        _ => Err(format!(
            "fixture requires {TAILSCALE_LOCAL_JOURNAL_ENV} and {TAILSCALE_GLOBAL_JOURNAL_ENV} together"
        )),
    }
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn verify_apply_journals(mount_path: &str, target: &str) -> Result<bool, String> {
    let Some((local, global)) = journal_paths()? else {
        return Ok(false);
    };
    let local_bytes = std::fs::read(&local).map_err(|error| {
        format!("local ownership journal was not published before apply: {error}")
    })?;
    let global_bytes = std::fs::read(&global).map_err(|error| {
        format!("global ownership journal was not published before apply: {error}")
    })?;
    if local_bytes != global_bytes {
        return Err("local/global ownership journals were not byte-identical before apply".into());
    }
    let runfile: serde_json::Value = serde_json::from_slice(&local_bytes)
        .map_err(|error| format!("ownership journal was malformed before apply: {error}"))?;
    let root = runfile
        .as_object()
        .ok_or_else(|| "ownership journal root must be an object".to_string())?;
    if root
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(2)
        || root.get("tailscale").and_then(serde_json::Value::as_bool) != Some(true)
    {
        return Err("apply requires a typed schema-v2 Tailscale ownership journal".to_string());
    }
    let ownership = root
        .get("tailscale_serve")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "apply requires typed tailscale_serve ownership".to_string())?;
    let token = mount_path
        .strip_prefix("/_ferric/")
        .expect("validated mount path always has the Ferric prefix");
    let port = proxy_target_port(target)?;
    let expected_remote = format!("https://{TAILSCALE_FQDN}{mount_path}/v1");
    let expected_local = format!("http://127.0.0.1:{port}/v1");
    let typed = ownership.get("version").and_then(serde_json::Value::as_u64) == Some(1)
        && ownership.get("token").and_then(serde_json::Value::as_str) == Some(token)
        && ownership.get("fqdn").and_then(serde_json::Value::as_str) == Some(TAILSCALE_FQDN)
        && ownership
            .get("https_port")
            .and_then(serde_json::Value::as_u64)
            == Some(u64::from(TAILSCALE_HTTPS_PORT))
        && ownership
            .get("mount_path")
            .and_then(serde_json::Value::as_str)
            == Some(mount_path)
        && ownership
            .get("proxy_target")
            .and_then(serde_json::Value::as_str)
            == Some(target)
        && ownership
            .get("remote_base_url")
            .and_then(serde_json::Value::as_str)
            == Some(expected_remote.as_str())
        && ownership
            .get("before_status_sha256")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|digest| lower_hex(digest, 64));
    if !typed
        || root.get("port").and_then(serde_json::Value::as_u64) != Some(u64::from(port))
        || root.get("base_url").and_then(serde_json::Value::as_str) != Some(expected_local.as_str())
    {
        return Err("ownership journal did not match the requested mount and target".to_string());
    }
    Ok(true)
}

fn append_tailscale_log(arguments: &[String], journals_ready: Option<bool>) -> Result<(), String> {
    let path = std::env::var_os(TAILSCALE_LOG_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| format!("fixture requires {TAILSCALE_LOG_ENV}"))?;
    let record = serde_json::json!({
        "invoked_filename": std::env::current_exe()
            .ok()
            .and_then(|path| path.file_name().map(|name| name.to_string_lossy().into_owned())),
        "argv": arguments,
        "pid": std::process::id(),
        "journals_ready_on_apply": journals_ready,
    });
    let mut line = serde_json::to_vec(&record)
        .map_err(|error| format!("could not serialize fixture Tailscale command log: {error}"))?;
    line.push(b'\n');
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("could not open fixture Tailscale command log: {error}"))?;
    log.write_all(&line)
        .and_then(|()| log.flush())
        .map_err(|error| format!("could not append fixture Tailscale command log: {error}"))
}

fn expected_host() -> String {
    format!("{TAILSCALE_FQDN}:{TAILSCALE_HTTPS_PORT}")
}

fn compatible_https_state(root: &serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
    if let Some(true) = root
        .get("AllowFunnel")
        .and_then(serde_json::Value::as_object)
        .and_then(|funnel| funnel.get(&expected_host()))
        .and_then(serde_json::Value::as_bool)
    {
        return Err("fixture refuses to mutate a Funnel-enabled host".to_string());
    }
    if let Some(port) = root
        .get("TCP")
        .and_then(serde_json::Value::as_object)
        .and_then(|tcp| tcp.get(&TAILSCALE_HTTPS_PORT.to_string()))
    {
        let port = port
            .as_object()
            .ok_or_else(|| "fixture TCP 443 state must be an object".to_string())?;
        if port.get("HTTPS").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err("fixture TCP 443 state is not compatible HTTPS".to_string());
        }
        for field in ["HTTP", "TCPForward", "TerminateTLS"] {
            if port.get(field).is_some_and(|value| match value {
                serde_json::Value::Null | serde_json::Value::Bool(false) => false,
                serde_json::Value::String(value) if value.is_empty() => false,
                _ => true,
            }) {
                return Err(format!("fixture TCP 443 has incompatible {field} state"));
            }
        }
    }
    Ok(())
}

fn apply_tailscale_path(state_path: &Path, mount_path: &str, target: &str) -> Result<(), String> {
    let mut state = read_tailscale_state(state_path)?;
    let root = state
        .as_object_mut()
        .expect("read_tailscale_state validates an object root");
    compatible_https_state(root)?;
    let tcp = root
        .entry("TCP")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "fixture TCP state must be an object".to_string())?;
    tcp.entry(TAILSCALE_HTTPS_PORT.to_string())
        .or_insert_with(|| serde_json::json!({"HTTPS": true}));
    let web = root
        .entry("Web")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "fixture Web state must be an object".to_string())?;
    let host = web
        .entry(expected_host())
        .or_insert_with(|| serde_json::json!({"Handlers": {}}))
        .as_object_mut()
        .ok_or_else(|| "fixture Web host state must be an object".to_string())?;
    let handlers = host
        .entry("Handlers")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "fixture Web handlers state must be an object".to_string())?;
    if handlers.contains_key(mount_path) {
        return Err("fixture refuses to replace an existing Serve path".to_string());
    }
    handlers.insert(mount_path.to_string(), serde_json::json!({"Proxy": target}));
    write_tailscale_state(state_path, &state)
}

fn loopback_target_is_healthy(target: &str) -> bool {
    let Ok(port) = proxy_target_port(target) else {
        return false;
    };
    let address = format!("127.0.0.1:{port}")
        .parse()
        .expect("valid loopback socket");
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(500)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    if stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut response = Vec::new();
    stream.read_to_end(&mut response).is_ok() && response.starts_with(b"HTTP/1.1 200 OK\r\n")
}

fn remove_tailscale_path(state_path: &Path, mount_path: &str) -> Result<(), String> {
    let mut state = read_tailscale_state(state_path)?;
    let root = state
        .as_object_mut()
        .expect("read_tailscale_state validates an object root");
    compatible_https_state(root)?;
    let handler = root
        .get_mut("Web")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|web| web.get_mut(&expected_host()))
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|host| host.get_mut("Handlers"))
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|handlers| handlers.get(mount_path))
        .ok_or_else(|| "fixture cannot remove an absent Serve path".to_string())?;
    let handler = handler
        .as_object()
        .ok_or_else(|| "fixture owned Serve handler must be an object".to_string())?;
    let target = handler
        .get("Proxy")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "fixture owned Serve handler must be an exact proxy".to_string())?
        .to_string();
    if handler.get("Text").is_some_and(|value| !value.is_null())
        || handler.get("Path").is_some_and(|value| !value.is_null())
    {
        return Err("fixture refuses to remove a combined Serve handler".to_string());
    }
    proxy_target_port(&target)?;
    if !loopback_target_is_healthy(&target) {
        return Err(format!(
            "fixture scoped off requires the recorded target {target} to remain HTTP-healthy"
        ));
    }
    let handlers = root
        .get_mut("Web")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|web| web.get_mut(&expected_host()))
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|host| host.get_mut("Handlers"))
        .and_then(serde_json::Value::as_object_mut)
        .expect("the handler lookup above proved this object path");
    handlers.remove(mount_path);
    write_tailscale_state(state_path, &state)
}

fn run_tailscale_fixture(arguments: &[String]) -> Result<(), String> {
    let request = parse_tailscale_request(arguments);
    let journal_check = match &request {
        Ok(TailscaleRequest::Apply { mount_path, target }) => {
            Some(verify_apply_journals(mount_path, target))
        }
        _ => None,
    };
    let journals_ready = journal_check
        .as_ref()
        .map(|result| result.as_ref().is_ok_and(|ready| *ready));
    append_tailscale_log(arguments, journals_ready)?;
    let request = request?;
    if let Some(result) = journal_check
        && !result?
    {
        return Err(
            "fixture refuses Serve apply without both pre-published ownership journals".to_string(),
        );
    }
    match request {
        TailscaleRequest::WhoAmI => {
            println!(r#"{{"Node":{{"Name":"{TAILSCALE_FQDN}."}}}}"#);
            Ok(())
        }
        TailscaleRequest::Status => {
            let path = tailscale_state_path()?;
            let state = read_tailscale_state(&path)?;
            println!(
                "{}",
                serde_json::to_string(&state)
                    .map_err(|error| format!("could not serialize fixture status: {error}"))?
            );
            Ok(())
        }
        TailscaleRequest::Apply { mount_path, target } => {
            let path = tailscale_state_path()?;
            apply_tailscale_path(&path, &mount_path, &target)
        }
        TailscaleRequest::Off { mount_path } => {
            let path = tailscale_state_path()?;
            remove_tailscale_path(&path, &mount_path)
        }
    }
}

fn record_invocation() -> std::io::Result<()> {
    let Some(marker) = std::env::var_os(INVOCATION_MARKER_ENV) else {
        return Ok(());
    };
    let executable = std::env::current_exe()?;
    let argv = std::env::args_os()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let invoked_filename = executable
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    let record = serde_json::json!({
        "executable": executable,
        "invoked_filename": invoked_filename,
        "argv": argv,
        "pid": std::process::id(),
    });
    let bytes = serde_json::to_vec_pretty(&record).map_err(std::io::Error::other)?;
    std::fs::write(marker, bytes)
}

fn argument_value(arguments: &[String], name: &str) -> Option<String> {
    arguments
        .windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

fn write_optional_marker(environment: &str, bytes: &[u8]) -> std::io::Result<()> {
    let Some(path) = std::env::var_os(environment) else {
        return Ok(());
    };
    std::fs::write(path, bytes)
}

fn respond(mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut request = [0_u8; 4096];
    let read = stream.read(&mut request)?;
    let request = String::from_utf8_lossy(&request[..read]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    let (status, body) = if path == "/health" {
        ("200 OK", r#"{"status":"ok"}"#)
    } else {
        ("404 Not Found", r#"{"error":"not found"}"#)
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
}

fn main() -> ExitCode {
    // This optional marker is test-only and intentionally precedes all argv
    // handling: either fake executable alias leaves evidence if it is invoked.
    if let Err(error) = record_invocation() {
        eprintln!("fixture could not record invocation: {error}");
        return ExitCode::FAILURE;
    }
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    match invoked_as_tailscale() {
        Ok(true) => {
            return match run_tailscale_fixture(&arguments) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("fixture Tailscale command failed: {error}");
                    ExitCode::FAILURE
                }
            };
        }
        Ok(false) => {}
        Err(error) => {
            eprintln!("fixture could not identify its executable alias: {error}");
            return ExitCode::FAILURE;
        }
    }
    if arguments.iter().any(|argument| argument == "--version") {
        println!("ferric lifecycle fixture 1");
        return ExitCode::SUCCESS;
    }

    let host = argument_value(&arguments, "--host").unwrap_or_else(|| "127.0.0.1".into());
    if host != "127.0.0.1" {
        eprintln!("fixture requires --host 127.0.0.1, received {host}");
        return ExitCode::FAILURE;
    }
    let Some(port) = argument_value(&arguments, "--port")
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port != 0)
    else {
        eprintln!("fixture requires a nonzero numeric --port");
        return ExitCode::FAILURE;
    };
    let Some(model) = argument_value(&arguments, "-m").map(PathBuf::from) else {
        eprintln!("fixture requires the ordinary -m MODEL argument");
        return ExitCode::FAILURE;
    };
    if !model.is_file() {
        eprintln!("fixture model must be a regular file: {}", model.display());
        return ExitCode::FAILURE;
    }
    let Some(_context_size) = argument_value(&arguments, "-c")
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value != 0)
    else {
        eprintln!("fixture requires a nonzero numeric -c context size");
        return ExitCode::FAILURE;
    };
    let Some(lifetime_token) = std::env::var_os(LIFETIME_TOKEN_ENV).map(PathBuf::from) else {
        eprintln!("fixture requires a guarded lifetime token");
        return ExitCode::FAILURE;
    };
    if !lifetime_token.is_file() {
        eprintln!(
            "fixture lifetime token is absent or nonregular: {}",
            lifetime_token.display()
        );
        return ExitCode::FAILURE;
    }

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => listener,
        Err(error) => {
            if error.kind() == std::io::ErrorKind::AddrInUse
                && let Err(marker_error) =
                    write_optional_marker(BIND_DIAGNOSTIC_ENV, ADDRESS_IN_USE_DIAGNOSTIC)
            {
                eprintln!("fixture could not record address-in-use diagnosis: {marker_error}");
            }
            eprintln!("fixture could not bind 127.0.0.1:{port}: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = listener.set_nonblocking(true) {
        eprintln!("fixture could not enable controlled cleanup: {error}");
        return ExitCode::FAILURE;
    }
    if let Err(error) = write_optional_marker(READY_MARKER_ENV, READY_MARKER) {
        eprintln!("fixture could not record readiness: {error}");
        return ExitCode::FAILURE;
    }
    loop {
        if !lifetime_token.is_file() {
            return ExitCode::SUCCESS;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if let Err(error) = thread::Builder::new()
                    .name("ferric-fixture-http".into())
                    .spawn(move || {
                        let _ = respond(stream);
                    })
                {
                    eprintln!("fixture could not dispatch an HTTP connection: {error}");
                    return ExitCode::FAILURE;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => {
                eprintln!("fixture listener failed: {error}");
                return ExitCode::FAILURE;
            }
        }
    }
}

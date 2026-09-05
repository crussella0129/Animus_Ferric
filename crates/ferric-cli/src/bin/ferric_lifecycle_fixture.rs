//! Model-free `llama-server` lifecycle fixture.
//!
//! This binary exists only behind the `lifecycle-fixture` feature. Integration
//! tests copy it to the platform's exact `llama-server` filename so Ferric's
//! real closed-engine launch, identity inspection, and teardown paths run
//! without loading a model.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

#[cfg(target_os = "linux")]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

const INVOCATION_MARKER_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_INVOCATION_MARKER";
const LIFETIME_TOKEN_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_LIFETIME_TOKEN";
const BIND_DIAGNOSTIC_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_BIND_DIAGNOSTIC";
const READY_MARKER_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_READY_MARKER";
#[cfg(target_os = "linux")]
const OWNER_PID_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_OWNER_PID";
#[cfg(target_os = "linux")]
const OWNER_START_TICKS_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_OWNER_START_TICKS";
const ADDRESS_IN_USE_DIAGNOSTIC: &[u8] = b"ferric-lifecycle-fixture:address-in-use:v1\n";
const READY_MARKER: &[u8] = b"ferric-lifecycle-fixture:ready:v1\n";

#[cfg(target_os = "linux")]
struct HarnessOwner {
    pidfd: OwnedFd,
}

#[cfg(target_os = "linux")]
impl HarnessOwner {
    fn acquire_from_environment() -> std::io::Result<Self> {
        let pid = std::env::var(OWNER_PID_ENV)
            .map_err(|_| std::io::Error::other("fixture owner PID is required"))?
            .parse::<libc::pid_t>()
            .map_err(|_| std::io::Error::other("fixture owner PID must be numeric"))?;
        if pid <= 1 {
            return Err(std::io::Error::other(
                "fixture owner PID must name a non-init process",
            ));
        }
        let expected_start_ticks = std::env::var(OWNER_START_TICKS_ENV)
            .map_err(|_| std::io::Error::other("fixture owner start ticks are required"))?
            .parse::<u64>()
            .map_err(|_| std::io::Error::other("fixture owner start ticks must be numeric"))?;
        let raw_pidfd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0_u32) };
        if raw_pidfd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let owner = Self {
            pidfd: unsafe { OwnedFd::from_raw_fd(raw_pidfd as libc::c_int) },
        };
        let observed_start_ticks = read_linux_start_ticks(pid)?;
        if observed_start_ticks != expected_start_ticks {
            return Err(std::io::Error::other(format!(
                "fixture owner generation changed: expected start ticks {expected_start_ticks}, observed {observed_start_ticks}"
            )));
        }
        if owner.exited()? {
            return Err(std::io::Error::other(
                "fixture owner exited during generation validation",
            ));
        }
        Ok(owner)
    }

    fn exited(&self) -> std::io::Result<bool> {
        let mut descriptor = libc::pollfd {
            fd: self.pidfd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut descriptor, 1, 0) };
        if result < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if result == 0 {
            return Ok(false);
        }
        if descriptor.revents & !(libc::POLLIN | libc::POLLHUP) != 0 {
            Err(std::io::Error::other(format!(
                "fixture owner pidfd returned invalid poll events {:#x}",
                descriptor.revents
            )))
        } else if descriptor.revents & (libc::POLLIN | libc::POLLHUP) != 0 {
            Ok(true)
        } else {
            Err(std::io::Error::other(format!(
                "fixture owner pidfd returned unexpected poll events {:#x}",
                descriptor.revents
            )))
        }
    }
}

#[cfg(target_os = "linux")]
fn read_linux_start_ticks(pid: libc::pid_t) -> std::io::Result<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let close = stat
        .rfind(')')
        .ok_or_else(|| std::io::Error::other("fixture owner stat lacked a command delimiter"))?;
    stat.get(close + 1..)
        .and_then(|suffix| suffix.split_whitespace().nth(19))
        .and_then(|field| field.parse::<u64>().ok())
        .ok_or_else(|| std::io::Error::other("fixture owner stat lacked numeric start ticks"))
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

    #[cfg(target_os = "linux")]
    let harness_owner = match HarnessOwner::acquire_from_environment() {
        Ok(owner) => owner,
        Err(error) => {
            eprintln!("fixture could not retain its test-harness owner: {error}");
            return ExitCode::FAILURE;
        }
    };

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
        #[cfg(target_os = "linux")]
        match harness_owner.exited() {
            Ok(true) => return ExitCode::SUCCESS,
            Ok(false) => {}
            Err(error) => {
                eprintln!("fixture could not observe its test-harness owner: {error}");
                return ExitCode::FAILURE;
            }
        }
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

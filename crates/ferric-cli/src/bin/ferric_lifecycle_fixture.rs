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

const INVOCATION_MARKER_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_INVOCATION_MARKER";
const LIFETIME_TOKEN_ENV: &str = "FERRIC_LIFECYCLE_FIXTURE_LIFETIME_TOKEN";

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

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("fixture could not bind 127.0.0.1:{port}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let lifetime_token = std::env::var_os(LIFETIME_TOKEN_ENV).map(PathBuf::from);
    if lifetime_token.is_some()
        && let Err(error) = listener.set_nonblocking(true)
    {
        eprintln!("fixture could not enable controlled cleanup: {error}");
        return ExitCode::FAILURE;
    }
    loop {
        if lifetime_token.as_ref().is_some_and(|path| !path.is_file()) {
            return ExitCode::SUCCESS;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = respond(stream);
            }
            Err(error)
                if lifetime_token.is_some() && error.kind() == std::io::ErrorKind::WouldBlock =>
            {
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => {
                eprintln!("fixture listener failed: {error}");
                return ExitCode::FAILURE;
            }
        }
    }
}

//! `ferric-valve` — serve Ferric's constrained decoding in front of a
//! llama.cpp server, for Hermes Agent's custom OpenAI-compatible endpoint.
//!
//! ```text
//! ferric-valve --upstream http://127.0.0.1:8080 --receipts receipts.jsonl
//! ```
//!
//! Point Hermes's custom provider `base_url` at `http://127.0.0.1:8090/v1`
//! and its `api_key` at the token the valve prints the path of. Every request
//! passes the network gate (sprint 127, INT-0015): this listener's `Host`, no
//! browser, the bearer token, JSON bodies.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use ferric_gate::{Gate, Token, default_token_path};
use ferric_valve::probe::probe_enforcement;
use ferric_valve::receipt::ReceiptSink;
use ferric_valve::server::{
    ValveConfig, ValveMode, check_loopback, default_listen, gated_router, serve,
    upstream_client_with_key,
};

#[derive(Parser)]
#[command(
    name = "ferric-valve",
    about = "Constrained-decoding valve between Hermes Agent and llama.cpp"
)]
struct Args {
    /// Upstream llama.cpp server origin, e.g. http://127.0.0.1:8080.
    #[arg(long)]
    upstream: String,
    /// Loopback address to listen on.
    #[arg(long, default_value_t = default_listen())]
    listen: SocketAddr,
    /// Append one JSON receipt per chat request to this file.
    #[arg(long)]
    receipts: Option<PathBuf>,
    /// Forward every request unchanged while recording receipts (the native
    /// comparison arm). Never constrains.
    #[arg(long)]
    record_only: bool,
    /// Minimum milliseconds between progress heartbeats on a constrained stream.
    #[arg(long, default_value_t = 2_000)]
    heartbeat_ms: u64,
    /// The valve's bearer-token file (created mode 0600 when missing).
    /// Default: `<state dir>/ferric/gate/valve.token`.
    #[arg(long)]
    token_file: Option<PathBuf>,
    /// A file holding the upstream llama.cpp server's API key (the file you
    /// passed to `llama-server --api-key-file`). Clients' credentials are
    /// never forwarded upstream.
    #[arg(long)]
    upstream_key_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    if let Err(message) = check_loopback(&args.listen) {
        eprintln!("ferric-valve: {message}");
        return ExitCode::from(2);
    }
    let Some(token_path) = args
        .token_file
        .clone()
        .or_else(|| default_token_path("valve"))
    else {
        eprintln!("ferric-valve: no state directory; pass --token-file");
        return ExitCode::from(2);
    };
    let (token, created) = match Token::load_or_create(&token_path) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("ferric-valve: {error}");
            return ExitCode::from(2);
        }
    };
    let upstream_key = match &args.upstream_key_file {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(text) => Some(text.trim().to_string()),
            Err(error) => {
                eprintln!("ferric-valve: cannot read {}: {error}", path.display());
                return ExitCode::from(2);
            }
        },
        None => None,
    };
    let mode = if args.record_only {
        ValveMode::RecordOnly
    } else {
        ValveMode::Constrained
    };
    if mode == ValveMode::Constrained {
        let client = match upstream_client_with_key(upstream_key.as_deref()) {
            Ok(client) => client,
            Err(error) => {
                eprintln!("ferric-valve: cannot build the upstream client: {error}");
                return ExitCode::FAILURE;
            }
        };
        if let Err(error) = probe_enforcement(&client, &args.upstream).await {
            eprintln!(
                "ferric-valve: refusing to serve constrained mode: {error}. \
                 Check that the upstream is a llama.cpp server that honors response_format."
            );
            return ExitCode::from(3);
        }
    }
    let receipts = match &args.receipts {
        Some(path) => match ReceiptSink::open(path) {
            Ok(sink) => sink,
            Err(error) => {
                eprintln!(
                    "ferric-valve: cannot open receipts file {}: {error}",
                    path.display()
                );
                return ExitCode::FAILURE;
            }
        },
        None => ReceiptSink::disabled(),
    };
    let upstream = args.upstream.clone();
    let config = ValveConfig {
        upstream: args.upstream,
        mode,
        heartbeat: Duration::from_millis(args.heartbeat_ms),
    };
    let listener = match tokio::net::TcpListener::bind(args.listen).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("ferric-valve: cannot listen on {}: {error}", args.listen);
            return ExitCode::FAILURE;
        }
    };
    let port = listener
        .local_addr()
        .map_or(args.listen.port(), |addr| addr.port());
    let gate = Gate::new(&token, port);
    let app = match gated_router(config, receipts, gate, upstream_key.as_deref()) {
        Ok(app) => app,
        Err(error) => {
            eprintln!("ferric-valve: cannot build the router: {error}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!(
        "ferric-valve: {} mode on http://{} -> {}",
        if mode == ValveMode::Constrained {
            "constrained"
        } else {
            "record-only"
        },
        listener
            .local_addr()
            .map_or_else(|_| args.listen.to_string(), |addr| addr.to_string()),
        upstream
    );
    eprintln!(
        "ferric-valve: bearer token {} {} (set it as Hermes's api_key)",
        if created { "minted at" } else { "read from" },
        token_path.display()
    );
    match serve(listener, app).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ferric-valve: server error: {error}");
            ExitCode::FAILURE
        }
    }
}

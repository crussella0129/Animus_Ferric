//! The `ferric` binary — a thin shim over the `ferric_cli` library, which owns
//! the command surface. The binary exists to supply argv[0] and the
//! `CARGO_BIN_NAME` the library records for its fixture-transport gate.

fn main() -> std::process::ExitCode {
    ferric_cli::run(env!("CARGO_BIN_NAME"))
}

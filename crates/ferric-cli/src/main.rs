//! The `ferric` binary — a thin shim over the `ferric_cli` library, which owns
//! the command surface.

fn main() -> std::process::ExitCode {
    ferric_cli::run()
}

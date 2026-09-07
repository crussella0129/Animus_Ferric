//! The `ferric-lifecycle-test` binary: byte-for-byte the same behavior as
//! `ferric`, but a distinct `CARGO_BIN_NAME` that the library records and uses
//! to activate the model-free fixture LocalAPI transport (`tailscale_localapi`,
//! under the `lifecycle-fixture` feature). The lifecycle CI tests spawn it by
//! name. Built only with `--features lifecycle-fixture`.

fn main() -> std::process::ExitCode {
    ferric_cli::run(env!("CARGO_BIN_NAME"))
}

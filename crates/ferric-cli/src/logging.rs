//! Harness-internal observability (sprint 72, ADR-063).
//!
//! Installs the process-wide `tracing` subscriber — the DIAGNOSTIC channel,
//! kept strictly distinct from the LLM JSONL trajectory (ADR-002). The trace
//! answers "what did the model do"; this answers "why did the harness do what
//! it did" (guard trips, retries, compaction, hook failures, tool timing).
//!
//! Three constraints shape it:
//! - **stderr only.** Several surfaces treat stdout as a machine channel
//!   (`ferric mcp` speaks JSON-RPC there; `ferric query`/`ferric launch` write
//!   their report there). Diagnostics MUST NOT pollute it — the subscriber
//!   writes to stderr.
//! - **Quiet by default.** With no `-v` and no env override the floor is WARN,
//!   so an ordinary run prints nothing extra.
//! - **`FERRIC_LOG` / `RUST_LOG` win.** A non-empty env filter (e.g.
//!   `FERRIC_LOG=ferric_loop=debug,ferric_tools=trace`) overrides the `-v`
//!   count entirely, so targeted per-crate debugging needs no rebuild.

use tracing_subscriber::EnvFilter;

/// Map a `-v` repetition count to a global level floor.
/// `0` → `warn` (quiet), `1` → `info`, `2` → `debug`, `3+` → `trace`.
pub fn level_str_for_verbosity(verbosity: u8) -> &'static str {
    match verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    }
}

/// Decide the env-filter string: a present, non-empty env value wins; else the
/// verbosity floor. Pure so it is unit-testable — the env reads live in `init`.
fn resolve_filter(env_val: Option<&str>, verbosity: u8) -> String {
    match env_val {
        Some(v) if !v.trim().is_empty() => v.to_string(),
        _ => level_str_for_verbosity(verbosity).to_string(),
    }
}

/// Install the process-wide `tracing` subscriber (stderr, quiet-by-default).
///
/// Uses `try_init`, so a redundant call (a test that already set a default, a
/// second entry point) is swallowed rather than panicking. A malformed env
/// filter falls back to `warn` instead of aborting the run.
pub fn init(verbosity: u8) {
    // `FERRIC_LOG` is the Ferric-specific knob; `RUST_LOG` is honoured as the
    // ecosystem convention. First non-empty one wins.
    let env_val = std::env::var("FERRIC_LOG")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| std::env::var("RUST_LOG").ok());
    let filter_str = resolve_filter(env_val.as_deref(), verbosity);
    let filter = EnvFilter::try_new(&filter_str).unwrap_or_else(|_| EnvFilter::new("warn"));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(true)
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbosity_maps_to_levels() {
        assert_eq!(level_str_for_verbosity(0), "warn");
        assert_eq!(level_str_for_verbosity(1), "info");
        assert_eq!(level_str_for_verbosity(2), "debug");
        assert_eq!(level_str_for_verbosity(3), "trace");
        // Saturates at trace — extra -v's don't wrap or panic.
        assert_eq!(level_str_for_verbosity(9), "trace");
    }

    #[test]
    fn env_value_overrides_verbosity() {
        // A concrete env filter beats the -v floor, even at -vvv.
        assert_eq!(
            resolve_filter(Some("ferric_loop=debug"), 3),
            "ferric_loop=debug"
        );
        assert_eq!(resolve_filter(Some("trace"), 0), "trace");
    }

    #[test]
    fn empty_or_absent_env_falls_back_to_verbosity() {
        assert_eq!(resolve_filter(None, 0), "warn");
        assert_eq!(resolve_filter(None, 2), "debug");
        // A blank/whitespace env var is treated as unset.
        assert_eq!(resolve_filter(Some(""), 1), "info");
        assert_eq!(resolve_filter(Some("   "), 1), "info");
    }
}

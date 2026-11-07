//! The availability gate the live Docker suites share.

use ferric_research::sandbox::check_available;

/// `true` when the daemon is reachable; prints a visible skip notice otherwise,
/// so a green run without Docker is never mistaken for a validated one.
pub fn docker_ready(test: &str) -> bool {
    if check_available() {
        return true;
    }
    println!("SKIP {test}: no Docker daemon — NOT validated by this run");
    false
}

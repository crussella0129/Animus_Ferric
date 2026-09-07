//! Process binary identity. The `ferric-lifecycle-test` binary runs identical
//! code to `ferric` under a distinct name so the lifecycle tests can drive a
//! model-free fixture LocalAPI transport; the running name is recorded once at
//! startup by [`crate::run`] and read where that fixture path is gated
//! (`tailscale_localapi`). This replaces a compile-time `env!("CARGO_BIN_NAME")`,
//! which is undefined when this code is compiled as a library rather than a bin.

use std::sync::OnceLock;

static BINARY_NAME: OnceLock<String> = OnceLock::new();

/// Record the running binary's name. Called once, as the first thing `run` does,
/// so every later reader observes the real identity.
pub(crate) fn set_binary_name(name: &str) {
    let _ = BINARY_NAME.set(name.to_string());
}

/// The pure identity predicate: the lifecycle-fixture binary alone matches.
#[cfg_attr(not(feature = "lifecycle-fixture"), allow(dead_code))]
pub(crate) fn name_is_lifecycle_fixture(name: &str) -> bool {
    name == "ferric-lifecycle-test"
}

/// Whether this process is the lifecycle-fixture binary. `false` before
/// `set_binary_name` runs — only library unit tests reach that state, and they
/// never carried a binary name, so the previous compile-time check was `false`
/// for them too.
#[cfg_attr(not(feature = "lifecycle-fixture"), allow(dead_code))]
pub(crate) fn is_lifecycle_fixture_binary() -> bool {
    BINARY_NAME
        .get()
        .is_some_and(|name| name_is_lifecycle_fixture(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_lifecycle_fixture_matches_only_the_fixture_binary() {
        assert!(name_is_lifecycle_fixture("ferric-lifecycle-test"));
        assert!(!name_is_lifecycle_fixture("ferric"));
        assert!(!name_is_lifecycle_fixture(""));
        assert!(!name_is_lifecycle_fixture("ferric-lifecycle-fixture"));
    }
}

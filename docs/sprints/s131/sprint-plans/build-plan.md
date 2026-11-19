# Sprint 131 Build Plan — Linux engine ownership for ordinary users

**Owner decisions (2026-11-19):**

- T-12907: option 1.
- T-13006: keep both tool paths.

**Change.** In `crates/ferric-cli/src/server_process.rs`, the Linux socket
inventory gains `Restricted`: the kernel refused to list a process's
descriptors (`PermissionDenied`). The ownership decision skips restricted
peers. Everything else stays:

- the engine's own descriptors must be readable;
- every listening socket on the port must be one of the engine's;
- a readable process sharing it gives `OwnedByOther`;
- any other read failure is still uninspectable.

**Why the rule keys on "may not list" rather than on user ID.** ssh-agent
runs as the user and disables tracing, so its descriptors are unreadable to
that user. A same-user rule would still have refused managed launches on
most desktops. The residual is the same either way: an unreadable process
could share the engine's own socket only if root, or something able to
trace the engine, put it there.

**Docs:** `docs/configuration.md` and `docs/server-configuration.md` state
the rule.

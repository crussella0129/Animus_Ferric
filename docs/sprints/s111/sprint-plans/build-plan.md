Finalized - DO NOT EDIT

# Sprint 111 Build Plan

1. Do not build an offline fallback or change product behavior for this sprint.
2. Remove stale registration safely and confirm the target loopback port is
   unused.
3. Launch the installed engine through the freshly built Ferric release with
   the exact local model and context settings.
4. Validate the process, listener, registrations, HTTP endpoints, real query
   artifacts, terminal trace reason, and side-effect-free trace verifier.
5. Exercise clean shutdown and record the exact live Monday runbook plus known
   latency/lifecycle caveats.

# Sprint 122 End-to-End Tests

Tested head: `9eabcbc`.

## Status: partial — the gate's observable behavior is proven; a full picker-interaction driver is not-yet-possible

The observable won't-fit outcome — a large model against a small machine
produces an annotated picker and a confirmation that names the numbers, and the
engine does not start on a bare Enter — is proven end to end **at the decision
boundary** by the T-12203 unit tests over the actual production helpers
(`fit_annotation`, `wontfit_confirm_prompt`, `model_fit`, `start_engine_notice`)
plus the `native_probe` integration test. Together they cover every SHALL
response with real inputs; `choose_model`'s remaining lines wire these helpers
using the exact `io.read` y/N pattern already proven by the source-tree guard.

A single driver that runs `choose_model`/`session_with` with a scripted terminal
and a *chosen* model size is **not-yet-possible without a new test seam**: a
model's `bytes` come from real on-disk file metadata (`models::scan`), and
`Startup::begin_in` is private to the `startup` module, so a test cannot present
a 20 GiB model without a 20 GiB file. This is an infrastructure limitation, not
a skipped assertion.

- **Unlocked by:** a future front-door test seam — either a `#[cfg(test)]`
  `Startup` constructor that accepts a synthetic model list, or threading the
  injectable `MemoryProbe` into `session_with` — recorded as a small follow-up
  under INT-0008. Neither changes this sprint's proven behavior.

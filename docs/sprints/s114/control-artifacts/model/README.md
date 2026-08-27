# Sprint 114 model acquisition controls

`model-spec.json` freezes the primary Qwen3.8-27B `UD-Q4_K_M` conversion and
the sole conditional `UD-Q3_K_XL` fallback. Both files come from the pinned
third-party Unsloth conversion revision; the underlying Qwen repository is
Apache-2.0.

`acquire-model.ps1` downloads only to an ignored `.part` path, verifies exact
length and SHA-256 with `verify-model.ps1`, and publishes the final filename
only after both checks pass. A bad file at the public filename is moved to a
recoverable ignored quarantine name. Every exit writes one atomic, uniquely
named, append-only structured result beside these controls, so a retry cannot
erase its predecessor. Git ignore/tracking policy is checked from the repository
root before publication. The Q3 path remains prohibited until it validates
the retained `runtime/q4-viability.json` decision produced by T-11409/E09-D;
an operator switch alone cannot authorize it. No 2-bit quant is in the frozen
specification.

The acquisition result is also emitted to stdout. A successful record combines
local verification with source provenance and Git-ignore/tracking proof; a
failure record retains the exact classified error without presenting any blob
as verified. The multi-gigabyte model itself remains under ignored `models/`
and is never committed.

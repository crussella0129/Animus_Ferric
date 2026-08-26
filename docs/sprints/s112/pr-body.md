## Summary

- establish a frozen 24-task internal autonomy corpus across ambiguity, durable recovery, and long-horizon repository work
- run every autonomy episode through the real `ferric query` process and server-backed OpenAI path; no mock/offline autonomy mode
- make pause/resume/resume-of-resume, structured clarification, refusal probes, and workspace-bound recovery explicit in traces, CLI, MCP, and HTTP
- gate `task_complete` on operator-named fixed-argv checks whose passing evidence is newer than the latest mutation
- retain exact Cartesian accounting, executable grading, traces, hashes/provenance, Wilson intervals, pass³, and paired repository-brief comparisons
- harden `ferric server up` against stale registrations, occupied ports, invalid model inputs, exited children, and TCP-only false readiness
- document ADR-103, the claim boundary, the next-sprint baseline plan, and operator usage

## Why

The prior benchmark and continuation paths could not reliably separate model failure from harness/configuration/trace/grader failure. That made long-horizon optimization vulnerable to false-green evidence. This sprint first makes the measurement and recovery contracts fail closed, then records a real small-model sample without promoting it into a general-agent claim.

## Live evidence

The post-freeze Qwen2.5-Coder-7B timed sample completed six infrastructure-clean episodes:

- passed: A05/current, A01/recovery, R02/recovery
- failed: R08/recovery, H01/recovery, H01/repository-brief
- targeted result: 3/6 contract and objective completion; Wilson 95% approximately 18.8%–81.2%
- all four expected continuation operations in A01/R02/R08 were observed and trace-linked, while R08 still failed its final objective
- the single repository-brief pair was failure/failure, so the brief is not enabled by default

This is an unbalanced six-episode sample—not the full 72-coordinate baseline, the 216-episode pass³ run, an external benchmark, or evidence of reliable general autonomy.

The hardened release also passed three independent process-cold `server doctor → up → status → down` lifecycles with distinct PIDs, exact loopback listener ownership, matching runfiles, HTTP model/context identity, and clean teardown. The server is left down.

## Validation

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features`
- 24/24 corpus tasks validate and every untouched seed fails its authoritative grader
- independent corpus and trace/recovery blocker audits
- release build with `backend-openai`
- template-hygiene tests
- three process-cold live server lifecycles
- retained and structurally verified live autonomy traces

## Known boundary / next sprint

`server down` still trusts a runfile PID without executable/start-time identity. Corpus v1 does not yet measure duplicate/collateral effects and emits null for that reserved field. Sprint 113 should collect the 72/216 baseline, then test read-before-edit recovery, bounded verification-guided repair, effect instrumentation, and safe teardown identity binding against held coordinates.

The owner must review and merge; this PR does not merge itself.

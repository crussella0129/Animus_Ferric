# Sprint 114 Meta

- **Sprint number:** 114
- **Book schema version:** 2
- **Start timestamp:** 2026-08-27T02:44:18Z
- **End timestamp:** (filled at Loop Phase)
- **Model:** Codex host model not exposed; evaluated local model Qwen3.8-27B UD-Q4_K_M with one gated UD-Q3_K_XL fallback
- **Exit status:** in-progress
- **Token count:** (filled at Loop Phase if observable)
- **Summary:** Select and attest a hardware-fitting current GGUF, run a no-repair Ferric Rust-app trial, and test Animus Sprint Loops compatibility layer by layer.
- **Intents:** [INT-0007 — Hardware-calibrated autonomous development](../../intents/INT-0007-hardware-calibrated-autonomous-development.md)
- **Completion evidence:** (filled at Loop Phase)

## Blockages

- **T-11409 — recoverable control-attestation defect:** the immutable
  `01-q4-32768` epoch-1 attempt loaded the exact Q4 model at context 32768 and
  tore down cleanly, but llama.cpp b10516 omitted `default_template_kwargs`
  from `/props`. The frozen attestor therefore could not prove effective
  `enable_thinking` and reasoning preservation and correctly recorded
  `infrastructure_blocked` before smoke or throughput. The offline verifier
  also binds the recorded launch `PATH` to its later verifier environment,
  which is not a portable archive check. Neither defect demonstrates Q4
  non-viability, so context 16384 and Q3 remain unauthorized and no Q4
  viability gate may be published. Preserve epoch 1 byte-for-byte; recover
  T-11409 only through a separately frozen, versioned control epoch with a
  non-inference behavioral proof of template defaults and environment-independent
  launch verification. The pinned-source analysis and recovery protocol are
  retained in
  [runtime-attestation-recovery.md](sprint-research/runtime-attestation-recovery.md).
- **T-11409 — second recoverable control-attestation defect:** the separately
  frozen epoch-2 `e02-01-q4-32768` attempt proved the four-arm template
  differential and every other managed-server property, but both online and
  offline checks compared Windows' retained basename command-line token with
  the independently resolved absolute executable path. The sole false
  predicate again produced `infrastructure_blocked` before smoke or
  throughput. The archive verifies cleanly and records clean teardown, but it
  is not viability evidence and authorizes neither a context retry nor Q3.
  Preserve epochs 1 and 2 byte-for-byte; recover only through epoch 3 with a
  shared Windows command-line parser, independently bound executable path/hash,
  exact tail-argument comparison, live-shaped positive fixtures, and negative
  path/argument tamper tests. The same recovery analysis records the retained
  evidence and protocol.

## Resolution

- **T-11409 — resolved:** epoch 3's `e03-01-q4-32768` attempt passed the
  corrected process-command attestation, real grammar-protocol nonce smoke,
  all three timed samples, and clean teardown. The Q4_K_M model remained at
  context 32,768 with 24 effective GPU layers and produced a retained median
  of `3.2064850358228254` decoded tokens per second. The frozen gate selected
  Q4 as viable; context 16,384 and Q3_K_XL were never authorized.
- Publication completed through separately frozen epochs 4 through 6 after a
  timestamp-protocol repair, a source-plan binding incident, and an
  `OrderedDictionary`/JSON-object incident. Those failed controls and the
  first failed epoch-6 pre-control self-test remain immutable evidence; no
  recovery epoch repeated model execution or changed the measured attempt.
- The final selection bundle passed all four runtime verification groups and
  cold teardown. Manifest
  `60cac4761f5276d05fe9f1be296925e4d9e0e7e667706564b8478bb792167f40`
  covers 213 runtime files without mismatch or residual staging state. See the
  retained [selection](control-artifacts/runtime/epoch-4/final/selection.json),
  [runtime verification](control-artifacts/runtime/epoch-4/final/runtime-verification.json),
  and [recovery analysis](sprint-research/runtime-attestation-recovery.md).

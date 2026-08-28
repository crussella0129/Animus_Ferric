Finalized - DO NOT EDIT

# Sprint 115 Test Plan

## Intent Traceability

| Intent | Acceptance criterion | Build task / EARS clause | Verification |
| --- | --- | --- | --- |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | Enabling evidence toward open AC-6; default compatibility | T-11414 / E14-A | `query_default_trace_root_is_compatible` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | Enabling evidence toward open AC-6; isolated evidence | T-11414 / E14-B | `query_external_trace_root_leaves_workspace_clean` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | Enabling evidence toward open AC-6; pre-mutation path safety | T-11414 / E14-C | `external_trace_root_precreate_rejection_matrix` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | Enabling evidence toward open AC-6; post-create path safety | T-11414 / E14-D | `external_trace_root_postcreate_rejection_matrix` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | Enabling evidence toward open AC-2/6; explicit continuation | T-11414 / E14-E | `external_trace_resume_requires_and_reuses_explicit_root` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | Enabling evidence toward open AC-2/6; truthful operator output | T-11414 / E14-F | `resume_hint_round_trips_in_documented_shell` |
| [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md), [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | INT-0007 AC-2; enabling evidence toward open INT-0008 AC-6 | T-11501 / E15-A | `V-11501-release-attestation` |
| [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md), [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | INT-0007 AC-2; enabling evidence toward open INT-0008 AC-6 | T-11501 / E15-B | `V-11501-release-behavior-probe` |
| [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md) | AC-3/4; lossless preparation | T-11502 / E16-A | `V-11502-quarantine-manifest-parity` |
| [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md) | AC-3/4; frozen bounded harness | T-11502 / E16-B | `V-11502-harness-and-sandbox-selftest` |
| [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md) | AC-2; measured host inventory | T-11503 / E17-A | `V-11503-cold-runtime-preflight` |
| [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md) | AC-2; managed-server behavior | T-11503 / E17-B | `V-11503-managed-runtime-attestation` |
| [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md) | AC-2/3; immutable handoff | T-11503 / E17-C | `V-11503-qualified-input-handoff` |
| [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md) | AC-3; exact first invocation | T-11410 / E10-A | `E2E-MH-RS01-first-segment` |
| [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md) | AC-3; forced continuation | T-11410 / E10-B | `E2E-MH-RS01-linked-resume` |
| [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md) | AC-4; no Codex repair/effect parity | T-11410 / E10-C | `E2E-MH-RS01-no-repair-reconciliation` |
| [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md) | AC-4; bounded execution | T-11410 / E10-D | `E2E-MH-RS01-sandbox-boundary` |
| [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md) | AC-3/6; independent outcome | T-11410 / E10-E | `E2E-MH-RS01-final-grade` |
| [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md) | AC-6; complete evidence | T-11412 / E12-A | `E2E-S115-evidence-manifest` |
| [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md) | AC-6; archive validity | T-11412 / E12-B | `E2E-S115-trace-and-effect-audit` |
| [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md) | AC-2/6; exact cold teardown | T-11412 / E12-C | `E2E-S115-exact-cold-teardown` |
| [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md), [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | INT-0007 AC-6; INT-0008 remains non-realized | T-11412 / E12-D | `E2E-S115-verdict-book-audit` |

INT-0007 AC-1, AC-5, and AC-7 retain Sprint 114 evidence and are not
modified. INT-0008 AC-2, AC-6, and AC-8 remain explicitly open: T-11414 is a
safe low-level prerequisite, not proof of the high-level or full-platform workflow.
INT-0008 AC-1, AC-3 through AC-5, AC-7, and AC-9 also remain future work.

## Unit Tests

### T-11414 pre-create path policy

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- `external_trace_root_resolves_nonexistent_tail` — E14-B: canonicalize the
  deepest existing ancestor and reconstruct a safe absent tail.
- `external_trace_root_precreate_rejects_equal` — E14-C.
- `external_trace_root_precreate_rejects_descendant` — E14-C.
- `external_trace_root_precreate_rejects_ancestor` — E14-C.
- `external_trace_root_precreate_rejects_existing_file` — E14-C.
- `external_trace_root_precreate_rejects_symlink_component` — E14-C, Unix.
- `external_trace_root_precreate_rejects_windows_junction` — E14-C, Windows.
- `external_trace_root_precreate_rejects_windows_case_alias` — E14-C, Windows.
- `external_trace_root_precreate_rejection_matrix` — E14-C: every case above
  asserts no requested directory or allocator/model artifact exists.

### T-11414 post-create revalidation

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- A test-only callback runs after `create_dir_all` and before final validation;
  its symbol is absent from non-test builds.
- `external_trace_root_postcreate_rejects_non_directory` — E14-D.
- `external_trace_root_postcreate_rejects_equal` — E14-D.
- `external_trace_root_postcreate_rejects_descendant` — E14-D.
- `external_trace_root_postcreate_rejects_ancestor` — E14-D.
- `external_trace_root_postcreate_rejects_symlink` — E14-D, Unix.
- `external_trace_root_postcreate_rejects_windows_reparse` — E14-D, Windows.
- `external_trace_root_postcreate_rejection_matrix` — E14-D: every substituted
  state refuses JSONL allocation and returns its exact diagnostic class.

### T-11414 documented-shell quoting

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- `powershell_quote_round_trips_argv` — E14-F, Windows: feed the emitted
  command to PowerShell and an argv-capture fixture; compare exact UTF-16-safe
  arguments including spaces, single/double quotes, `$`, backticks, `;`, and
  `&`.
- `posix_sh_quote_round_trips_argv` — E14-F, Unix: feed the emitted command to
  `/bin/sh` and the same argv-capture fixture; compare exact bytes for spaces,
  quotes, `$`, backticks, `;`, and `&`.

## Integration Tests

### Query CLI integration

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- `query_default_trace_root_is_compatible` — E14-A: existing mock fresh/resume
  behavior and `.ferric/trace` location remain unchanged.
- `query_external_trace_root_leaves_workspace_clean` — E14-B: a mock query
  writes externally and leaves no workspace `.ferric`.
- `external_trace_root_precreate_rejection_matrix` — E14-C: black-box invalid
  roots fail before any trace, requested directory, or mock artifact.
- `external_trace_resume_requires_and_reuses_explicit_root` — E14-E: omission
  fails without mutation; repetition creates an external continuation with the
  correct `resumed_from` link and no workspace `.ferric`.
- `resume_hint_round_trips_in_documented_shell` — E14-F: execute the emitted
  PowerShell or POSIX-`sh` command against an argv-capture fixture and compare
  `query`, resume trace, workspace, and trace root exactly.
- `query_help_documents_trace_dir` — E14-A/E14-E/E14-F: help names the default,
  explicit-resume rule, disjoint/reparse restrictions, and supported shell.

### Release qualification

- **Intents:** [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md),
  [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- `V-11501-release-attestation` — E15-A: `cargo fmt --check`, clippy, targeted
  and workspace tests pass; the backend-enabled binary, source commit, hash,
  version, and help capture agree.
- `V-11501-release-behavior-probe` — E15-B: the built binary passes four mock
  fresh/resume default/external probes with exact path and link assertions.

### Frozen harness and sandbox preparation

- **Intent:** [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md)
- `V-11502-quarantine-manifest-parity` — E16-A: verify each source and
  destination is within its named root; record every entry and regular-file
  hash before the move; move the entire exact root; regenerate the destination
  manifest; require identical relative paths, types, sizes, and hashes. No
  recursive delete occurs and all bytes remain under the retained quarantine.
- `V-11502-harness-and-sandbox-selftest` — E16-B: all frozen hashes match;
  positive/negative harness self-tests retain expected results; candidate Git
  is standalone; WSL/Bubblewrap proves network-disabled execution; every root
  recreated by self-tests is manifested and moved losslessly under the same
  quarantine contract; and all four canonical roots are absent immediately
  before handoff, or the gate fails before inference.

### Managed runtime integration

- **Intent:** [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md)
- `V-11503-cold-runtime-preflight` — E17-A: assert every named timestamp,
  physical/committed-memory, GPU, exact-image process/command-line, owned-PID,
  configured/owned listener, local/global runfile, model, engine, WSL,
  Bubblewrap, and no-network field is present; assert no unrelated PID receives
  a stop signal.
- `V-11503-managed-runtime-attestation` — E17-B: the exact selected coordinate
  passes owned launch, health/models/properties, grammar nonce smoke, bounded
  throughput, and archive verification or stops before app inference.
- `V-11503-qualified-input-handoff` — E17-C: the running server identity used
  by the app exactly matches the attested identity and no fallback, second
  download, or restart occurs.

## End-to-End Tests

- **Status:** possible
- `E2E-MH-RS01-first-segment` — E10-A: candidate seed and invocation hashes
  match; the exact one-turn query produces an incomplete retained trace or a
  truthfully classified alternate stop.
- `E2E-MH-RS01-linked-resume` — E10-B: the explicit external-root continuation
  is linked, uses the 27-turn budget, and never creates candidate `.ferric`.
- `E2E-MH-RS01-no-repair-reconciliation` — E10-C: before/after candidate
  inventories, trace effects, Git state, and journal prove no Codex write and
  attribute every change.
- `E2E-MH-RS01-sandbox-boundary` — E10-D: each model-authored execution and
  `run_check` is observed inside the frozen network-disabled sandbox; absence
  fails closed.
- `E2E-MH-RS01-final-grade` — E10-E: fresh in-session check evidence and all
  seven frozen dimensions are retained with a typed terminal outcome.
- `E2E-S115-evidence-manifest` — E12-A: every required artifact and failed or
  partial attempt appears in a self-verifying manifest.
- `E2E-S115-trace-and-effect-audit` — E12-B: all trace, hash, allowed-path, and
  effect/tree validators agree or the report names exact mismatches.
- `E2E-S115-exact-cold-teardown` — E12-C: each of the five named disposable
  roots, owned server process, listener, and both runfiles is absent, while
  `models/`, committed evidence, and the preservation quarantine remain.
- `E2E-S115-verdict-book-audit` — E12-D: intent state and capability language
  match evidence; a named backlog task orders the full INT-0008 workflow after
  this trial without claiming AC-2 or AC-8.

## Test Execution Order

1. T-11414 unit, CLI, shell-round-trip, formatter, and clippy tests.
2. T-11501 full workspace and backend-enabled release gates/probes.
3. T-11502 lossless quarantine, frozen-harness self-test, and sandbox gate.
4. T-11503 cold preflight and exact managed-runtime qualification.
5. T-11410 first segment, linked continuation, final grader, and no-repair
   reconciliation.
6. T-11412 manifest/trace/effect audit, exact teardown, Book audit, and Sprint
   Loop Test-phase validators.

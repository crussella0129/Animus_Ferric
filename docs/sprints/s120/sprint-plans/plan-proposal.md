# Sprint 120 — human-first review/refactor proposal

**Status: awaiting owner approval; not finalized.** This is Plan scratch, not
the canonical build/test plans. Those remain empty until approval. No source
changes are authorized by this file and no implementation has begun.

The installed Sprint Loops 0.22.0 adapter requires plan approval. Its
Claude-specific Plan Mode APIs are not exposed by this Codex host, so the
substantive boundary is explicit source freeze plus owner approval, not a fake
tool invocation. Preliminary independent review of this proposal does not
replace the canonical post-approval Plan critic and atomic lock.

## Build plan

### Intended human experience

1. Run `cargo r` from the repository, or installed `ferric` in a work folder.
2. Ferric finds configured/managed resources. If it needs a local engine, it
   finds regular GGUF models in the workspace's `models` directory and the
   installed closed `llama-server` engine. Ask for a model only when ambiguous.
3. Choose ask-only conversation or explicitly permit controlled file work in
   the displayed folder. Plain input then does that job; no `/do` vocabulary
   is required in the new human session. A resource commitment confirmation,
   when needed, is the third and last setup decision.
4. Short progress states explain loading and readiness. Technical fields are
   automatic conservative defaults or existing explicit config, not questions.
   Model preference persists; broad mutation consent does not. Quit/EOF/cancel
   stops and reaps a newly started engine, never a borrowed server.

Prepared-host means an already-installed compatible engine plus a local model,
or an explicitly configured/identity-verified ready server. This sprint does
not implement clean-host model/engine downloads, a hardware-fit optimizer,
automatic repair of ambiguous old registrations, or complete resumable app
orchestration. Absent resources are reported honestly, with one short supported
next action. No model download is planned; future acquisitions stay in `models`.

### Intents and scope

- [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md): active;
  front-door/startup/provider portions of AC-12 plus prepared-host portions of
  AC-1/3/5/6/7/9/10/11. Whole-session work-mode cancellation, AC-2/4/8 and the
  remaining portions are not claimed complete.
- [INT-0006](../../../intents/INT-0006-truthful-policy-contract.md): proposed;
  advance to planned only after approval; AC-5/6 configuration increment, not
  completion of the public inert-policy-field migration.
- [INT-0005](../../../intents/INT-0005-safe-multilanguage-syntax-admission.md):
  proposed; advance to planned only after approval; maintain Python AC-1–5
  admission behavior across the merged dependency change, no new language claim.
- [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md):
  dependency only. Readiness is not capability qualification or application success.

### Schema tree

- Human-first prepared-host session
  - T-12001: restore source build without weakening Python admission
  - T-12002: validated workspace-bound configuration
  - T-12003: reusable owned foreground preparation and model preferences
  - T-12004: default human entry point and compatible expert access
  - T-12005: responsive, byte-correct provider I/O
  - T-12006: journey qualification and concise documentation

### T-12001: Repair the merged RustPython 0.5 adapter

- **Intent:** INT-0005, AC-1–5 (Python maintenance only).
- **Touches:** `crates/ferric-tools/src/builtin/check_syntax.rs`,
  `crates/ferric-tools/tests/controlled_mutations.rs`.
- **Depends on:** none.
- **Success criterion (EARS):**
  - **E01-A:** WHEN valid, invalid, context-invalid, generic-guarded, or injected
    unsupported Python candidates are checked, THEN admission SHALL retain the
    typed valid/invalid/unchecked distinctions without subprocesses or files.
  - **E01-B:** WHEN `except*` is checked on RustPython 0.5, THEN it SHALL be valid,
    and controlled writes SHALL still atomically reject valid-to-invalid changes.
- **Notes:** no dependency rollback. Match only NotImplementedYet as unchecked;
  retain panic containment and conservative generic-syntax qualification guard.
  Parser identity/diagnostic provenance must name the actual pinned 0.5 adapter;
  the admission matrix asserts that identity rather than retaining a stale label.

### T-12002: Make configuration predictable and fail closed

- **Intent:** INT-0006 AC-5/6; INT-0008 AC-12.
- **Touches:** `crates/ferric-cli/src/config.rs`, `backend.rs`, config-consuming
  CLI surfaces (`query.rs`, `mcp.rs`, `api.rs`, `chat.rs`, `icm.rs`, bench/autonomy
  where applicable), their tests and configuration reference.
- **Depends on:** T-12001 for executable tests.
- **Success criterion (EARS):**
  - **E02-A:** WHEN an optional config file is absent, THEN loading SHALL preserve
    documented precedence/defaults; WHEN a present file is unreadable, malformed
    or contains an invalid known enum/numeric value, THEN every consumer SHALL
    fail before provider requests, hooks or mutations without revealing secrets.
  - **E02-B:** WHEN a surface selects workspace B while the current directory is
    A, THEN real-provider discovery SHALL use B and preserve explicit endpoint
    precedence and conflict/unverifiable refusal.
  - **E02-C:** WHEN config disables streaming or CLI sets `--no-stream`, THEN both
    chat talk and controlled turns SHALL use nonstreaming; omitted resume harness
    selection SHALL still inherit its source trace.
- **Notes:** typed fallible loader, finite positive params/context and finite
  supported temperature/ring checks at effective-setting boundaries. Preserve
  legacy tolerated unknown fields; do not blanket-deny them as a shortcut.
  Existing valid-config API reload timing stays unchanged in this increment;
  R08's snapshot-vs-reload contract remains T-12022. Direct library callers that
  construct invalid RunPolicy remain that same explicit follow-up, not falsely fixed.

### T-12003: Prepare and own a foreground local-model session

- **Intent:** INT-0008 AC-3/5/6/7/10/12 (prepared-host portions).
- **Touches:** new `crates/ferric-cli/src/startup.rs` and narrowly factored startup
  modules, existing `server.rs`/`server_process.rs` reusable primitives,
  `backend.rs`, CLI Cargo dependencies and source-defined startup fixtures.
- **Depends on:** T-12002; T-12005 before interactive acceptance.
- **Success criterion (EARS):**
  - **E03-A:** WHEN explicit configuration or Ready managed discovery identifies
    a backend, THEN preparation SHALL borrow it, bind credentials only to their
    explicitly configured endpoint, and issue no teardown for it; WHEN managed
    discovery is stale/conflicting/degraded/unverifiable, THEN preparation SHALL
    return actionable refusal without launch, registration deletion or signalling.
  - **E03-B:** WHEN a prepared host has an available model and compatible engine,
    THEN preparation SHALL launch only the closed loopback engine command in a
    retained ProcessTree, verify exact listener ownership and bounded readiness,
    and return a usable session without publishing a detached managed runfile.
  - **E03-C:** WHEN readiness fails, ownership mismatches, startup is cancelled,
    the session exits or unwinds, THEN every owned child SHALL be terminated and
    proved reaped through the shared checked cleanup contract before success;
    borrowed resources and unrelated listeners SHALL remain untouched.
  - **E03-D:** WHEN concurrent invocations target one workspace or a preference
    write is interrupted, THEN coordination SHALL admit only one local startup
    and leave the prior valid preference readable without overwriting expert
    config; malformed/stale preference SHALL trigger explicit re-selection or
    refusal, not silent authority changes.
  - **E03-E:** WHEN model metadata is absent, malformed, oversized, delayed or
    redirected, THEN startup SHALL enforce a five-second per-probe timeout,
    one-MiB streaming body cap, no redirects, and one finite startup deadline of
    180 seconds; trained context/model names SHALL not imply measured capability,
    automatic resource fit, or permission to enlarge tool authority.
  - **E03-F:** WHEN explain is requested, THEN it SHALL perform no network,
    subprocess, download, write or lock-file creation and SHALL disclose the
    selected resource, tentative defaults, ownership and expected effects.
- **Notes:** compose `server::command`, existing managed-state classification,
  native retained listener proof and `ferric_process::ProcessTree`, not shell
  command strings or another PID-only lifecycle manager. Foreground ownership
  is explicit; it does not call generic `server down` on exit. Acquire a
  crash-released OS lock for the selected workspace and protect preference
  publication against symlinks and concurrent updates. Coordinate current
  preference identity on every launch; never reuse it as a qualification profile.
  A basic resource check/explicit warning is not a fit guarantee. Persist no API
  keys or per-session edit consent. No L0-L6 benchmark is implicitly invoked.
  A new startup module must fail closed on native targets lacking exact listener
  authority; existing explicitly configured backends remain usable there.
  Represent borrowed and owned backends as distinct types. Pin owned host to
  `127.0.0.1`, disable Tailscale and set engine stdin to null. Launch the child
  with native no-window settings on Windows. Retain its exact process
  generation immediately after spawn; check liveness and exact listener identity
  before and after health/model probes, then explicitly pass its endpoint to the
  provider. Do not call a healthy foreign port owned or use the literal `default`
  model name when the endpoint advertises an actual model ID. Keep the OS lock
  until checked scope cleanup finishes; do not unlink the persistent lock file.
  Bound model directory entries/metadata count as well as response bytes, and
  retain only bounded engine diagnostics without an unbounded pipe-reader join.
  The coordination guarantee is one workspace, not global GPU exclusivity;
  different workspaces may compete for resources and no auto-fit claim is made.
  Existing Unix cooperative-group/SIGKILL limits remain explicit. Do not
  implicitly install the test-only parent watcher or process-wide subreaper.

### T-12004: Make ordinary launch the human interface

- **Intent:** INT-0008 AC-1/9/11/12.
- **Touches:** root `Cargo.toml`, CLI `Cargo.toml`, `main.rs`, `chat.rs`, startup
  presentation, `tests/cli.rs`, new human-journey tests, README/operator docs.
- **Depends on:** T-12002, T-12003, T-12005 before accepted interactive execution.
- **Success criterion (EARS):**
  - **E04-A:** WHEN normal default-feature `cargo r`/`ferric` runs on a terminal
    with prepared resources, THEN it SHALL reach an input-ready session after
    at most three meaningful decisions and zero technical-setting questions.
  - **E04-B:** WHEN no arguments are provided without a terminal, THEN the CLI
    SHALL print at most twelve lines of useful welcome, exit zero and create no
    state, network or processes; malformed explicit commands SHALL still exit 2.
  - **E04-C:** WHEN ask-only is selected, THEN no model output SHALL reach tool
    dispatch or hooks; WHEN folder work is explicitly selected, THEN ordinary
    objective text SHALL use the existing Evidence-controlled loop and workspace
    guard, with conservative tool ceiling and no implicit shell/ICM authority.
    A later session SHALL ask for folder authority again.
  - **E04-D:** WHEN the model is selected and successfully prepared, THEN repeat
    use SHALL reuse that valid preference without asking for model metadata;
    decline, EOF and interrupted setup SHALL exit cleanly without a technical
    error avalanche, and failures SHALL show one next safe action.
  - **E04-E:** WHEN top-level help or expert commands are requested, THEN common
    help SHALL expose no more than four primary actions (`run`, `status`,
    `explain`, `advanced`) while original command spellings and machine-readable
    formats remain compatible; explicit `--no-default-features` builds SHALL
    retain a truthful backend-unavailable welcome and their mock expert paths.
- **Notes:** normal source build includes the OpenAI-compatible backend and the
  workspace selects the CLI as default member. `run` with no objective opens the
  same session; optional one-shot objective requires explicit `--allow-edits`
  for folder mutation when no interactive consent is possible. Default ask mode
  is structural, not an incorrect assumption that Ring 0 means read-only.
  No `/run`/`!` shell passthrough in the new human-mode parser; old `chat`
  behavior remains the advanced compatible interface. Status describes only
  this increment's configuration/resource/session state, not invented completed
  workflow checkpoints. Verbose diagnostics may retain detail off the default path.

### T-12005: Make provider cancellation and Unicode streaming reliable

- **Intent:** INT-0008 AC-6/12; selects the bounded R04/R16 portion of T-12021.
- **Touches:** `crates/ferric-provider/src/openai.rs`, provider source-defined
  HTTP fixtures/tests, startup/chat cancellation integration as needed.
- **Depends on:** T-12001 for executable workspace integration.
- **Success criterion (EARS):**
  - **E05-A:** WHEN cancellation arrives during headers, error body, nonstream
    JSON or stream reads, THEN provider work SHALL stop within two seconds in
    bounded fixtures without continuing the request or leaking the server task;
  the front door SHALL then execute E03-C cleanup.
  - **E05-B:** WHEN an SSE event containing multibyte prose or tool JSON is split
    at any byte boundary, THEN decoding SHALL preserve exactly the unsplit
    content; malformed UTF-8 SHALL be reported, not silently replaced inside
    protocol JSON. Existing ASCII and DONE behavior SHALL remain compatible.
- **Notes:** reusable cancellation semantics, not a UI timer hiding a running
  provider. Test TCP tasks are joined with finite lifetimes. Preserve valid
  API response schemas and existing authentication/streaming behavior.

### T-12006: Qualify the human journey and publish honest documentation

- **Intent:** INT-0008 AC-9/11/12 and the affected INT-0005/0006 criteria above.
- **Touches:** source-defined journey/lifetime tests, required CI feature
  matrix, README/commands/configuration docs, Sprint 120 evidence and Book work.
- **Depends on:** T-12001 through T-12005.
- **Success criterion (EARS):**
  - **E06-A:** WHEN the prepared-host mock journey covers first run, repeat,
    consent denial, absent resources, conflicts, concurrency, startup/request
    cancellation and exit, THEN assertions SHALL verify decisions, output,
    workspace effects, exact borrowed/owned lifetime and no unreaped children.
  - **E06-B:** WHEN documentation and quality gates are evaluated, THEN default
    `cargo r` SHALL be the first supported command; README SHALL contain no new
    sprint-history ledger, fmt/clippy/tests and required CI SHALL pass at the
    exact implementation head, and deferred review findings SHALL remain visible.
  - **E06-C:** WHEN one bounded real-model prepared-host attempt is made through
    source-aware Cargo execution, THEN retained evidence SHALL identify actual
    model/runtime/settings, time to input/response, every human decision and
    verified owned cleanup; failure SHALL remain failure and SHALL NOT be
    repaired by manual process killing or counted as medium-horizon success.
- **Notes:** the source harness chooses existing resources through the same
  startup API, owns all fixtures/children and handles cancellation. No direct
  target executable invocation, visible helper windows, or ad-hoc background
  proof. Native Windows is the local acceptance target; Linux source fixtures
  run in CI with explicit supported identity conditions. No macOS/native
  hardware parity claim. Real-model acceptance is required for a success claim;
  failure takes the skill's failed-Test path rather than an unearned green PR.

### Explicit non-goals and retained risks

R03 ICM child symlink containment is T-12020, high priority and not reachable
through an added ICM authority grant. R08/direct-library R09/R10/R11 remain
T-12022/T-11406; R12/R13/R15 remain T-12023 plus existing measurement work;
R14/R17/R18 remain T-12024; R19/R20/R21 remain T-12025. Do not claim these fixed.
Provider R04/R16 is selected because waiting/cancellation and broken streamed
text directly undermine the human session. New code may not compose known
unbounded doctor/shell/Git operations as setup probes. Existing work-mode Git
snapshot waits remain a documented risk, not hidden as a solved provider issue.
Operator documentation must state that cancellation is bounded during startup
and provider requests, but an existing controlled-turn Git snapshot may still
block cancellation until Git returns. Therefore this sprint cannot claim full
AC-12 or universal two-second cancellation across work mode. T-12024 is required
before removing that limitation; the clause matrix covers its named phases only.
Broader local-model acquisition, calibrated fit, resume/status completeness,
skill compatibility and the frozen medium-horizon app trial remain separately
ordered work. This is a bounded refactor, not an unqualified repository seal.

## Test plan

### Intent traceability and named tests

These names are planned assertions, not already implemented tests. Each row
must receive its exact invocation, source location, result and immutable head
in Test evidence. Aggregate suite counts do not replace the clause matrix.

| Intent / criteria | Clause | Named verification |
|---|---|---|
| INT-0005 AC-1/3/4/5 | E01-A | `python_05_admission_matrix`; `unsupported_codegen_remains_unchecked`; `syntax_check_has_no_external_side_effects` |
| INT-0005 AC-2 | E01-B | `except_star_is_valid`; `controlled_mutation_python_05_transition_matrix` |
| INT-0006 AC-5/6 | E02-A | `config_absence_and_precedence`; `present_invalid_config_blocks_all_consumers`; `config_errors_redact_credentials`; `invalid_effective_numbers_rejected` |
| INT-0006 AC-6 | E02-B | `selected_workspace_drives_real_provider`; `explicit_endpoint_and_ambiguous_discovery_matrix` |
| INT-0006 AC-5/6 | E02-C | `chat_effective_stream_matrix`; `omitted_resume_harness_inherits` |
| INT-0008 AC-3/6/7/12 | E03-A | `startup_borrows_ready_server`; `startup_ambiguous_registration_is_nonmutating`; `startup_credentials_stay_endpoint_bound` |
| INT-0008 AC-6 | E03-B | `startup_owned_runtime_reaches_ready`; `startup_listener_identity_mismatch_refuses` |
| INT-0008 AC-6/7 | E03-C | `startup_cleanup_fault_matrix`; `startup_cancellation_reaps_scope`; `borrowed_server_survives_session_exit` |
| INT-0008 AC-3/10 | E03-D | `startup_concurrent_invocations_serialize`; `preferences_atomic_and_symlink_safe`; `stale_preferences_do_not_reuse_qualification` |
| INT-0008 AC-6/10/12 | E03-E | `startup_probe_limits_matrix`; `metadata_does_not_promote_authority` |
| INT-0008 AC-5 | E03-F | `startup_explain_is_read_only` |
| INT-0008 AC-1/12 | E04-A | `human_first_run_decision_budget`; `cargo_default_launch_selects_real_cli` |
| INT-0008 AC-12 | E04-B | `no_args_non_tty_welcome_is_nonmutating`; `malformed_explicit_commands_remain_usage_errors` |
| INT-0008 AC-6/12 | E04-C | `human_ask_never_dispatches`; `human_work_requires_scoped_consent`; `human_text_is_not_shell`; `new_session_does_not_inherit_edit_consent` |
| INT-0008 AC-10/12 | E04-D | `human_repeat_reuses_model`; `human_decline_eof_and_errors_are_bounded` |
| INT-0008 AC-1/11 | E04-E | `primary_help_is_compact`; `advanced_original_commands_compatible`; `no_default_features_welcome_and_mock_compatibility` |
| INT-0008 AC-6/12 | E05-A | `provider_cancellation_all_response_phases`; `human_cancel_during_request_reaps_owned_engine` |
| INT-0008 AC-12 | E05-B | `sse_unicode_every_split`; `sse_malformed_utf8_reports_error`; `sse_ascii_done_compatibility` |
| INT-0008 AC-9/12 | E06-A | `human_journey_e2e_matrix` (composes E02–E05, checks every fixture child reaped) |
| affected INT-0005/6/8 | E06-B | `source_quality_and_feature_matrix`; `first_run_docs_match_cli`; template-hygiene and existing command regression suites |
| INT-0008 AC-6/9/12 partial | E06-C | `real_model_prepared_host_journey` (source-owned live acceptance, separate from mock result) |

### Unit tests

Pure loader/precedence/error classification, bounded model-selection and
presentation reducers, preference validation, permission/mode routing,
incremental SSE parsing, and injected Python compiler results cover the named
unit assertions above. Inject environment/path/clock/probe collaborators rather
than mutate global current directory or print secrets. For unreadable config,
include deterministic injected IO errors as well as an actual directory-at-file
fixture; do not rely only on platform permission bits.

### Integration tests

Source-defined HTTP fixtures cover ready/stalled/redirected/oversized endpoints,
credentials and Unicode. Source-defined engine fixtures run only under Cargo-
driven tests, using the shared process owner and checked reaping. A real
concurrency barrier verifies one workspace startup winner; assertions cover
preference failure before/after publication, symlink refusal and borrowed
resource preservation. Selected-workspace tests use two explicit temp roots,
not global chdir. Existing query/MCP/API/chat/ICM configuration and resume tests
remain regression coverage, with protocol errors asserted in their own forms.

### End-to-end tests

**Status: possible for this prepared-host increment.** The source-owned journey
matrix exercises the same orchestration and injected terminal IO used by the
real front door, plus Cargo-driven process-level no-argument/help/invalid-args
and native lifecycle cases. Input transcript assertions count model/authority/
resource decisions, reject technical prompts, and verify observed file effects.
Do not call a parser snapshot an interactive end-to-end test. Include actual
terminal input where the host supports it; retain transcript and explain any
terminal-fixture limitation rather than substituting non-TTY welcome success.

The live test uses an existing local model with no downloads and a maximum
180-second startup plus one 120-second conversation attempt. Evidence records
the actual duration and any failure. A source-owned test controls cancellation
and proves reaping; do not manually terminate process leftovers. A short response
is usability evidence, not proof of long-horizon coding accuracy.

Full clean-host acquisition/calibration, complete interrupted workflow resume,
native platform parity and model-built application qualification remain unlocked
by INT-0007/INT-0008's named work (T-11505–T-11509, T-11506/T-11410/T-11412),
not silently waived by this narrower feasible E2E scope.

### Quality gates and sprint closeout

Run affected tests first after coherent changes, then `cargo fmt --all --check`,
workspace clippy with warnings denied, workspace tests with backend enabled,
CLI no-default-feature tests, and the lifecycle-fixture tests required by the
existing CI. Every child-producing command is source-aware and bounded. Retain
failed attempts, exact head/commands/results and cleanup evidence. Perform the
independent Test critique, Loop reconciliation and an additional independent
post-Loop adversarial pass. Only then commit/push, confirm remote head and
`origin/main..dev` contains this sprint alone, open one dev-to-main PR, verify
its commit count and required CI, restore/hash-check the protected user edit,
and stop for the owner to merge. Never merge the PR or label a failed live
qualification as successful sprint acceptance.

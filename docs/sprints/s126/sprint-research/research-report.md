# Sprint 126 Research Report

## Intents Reviewed
- [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md) — reviewed, unchanged; relevance: its negative result is cited by Amalgam as lesson L-11 and by INT-0014 as a component not to revive; current state: abandoned (terminal, not rewritten).
- [INT-0002](../../../intents/INT-0002-operator-authorized-default-verification.md) — revised; relevance: default verification for Ferric's own tools, which Hermes now owns; current state: abandoned (direction change).
- [INT-0003](../../../intents/INT-0003-requirement-evidenced-completion.md) — revised; relevance: a completion ledger for Ferric's loop, which Hermes now owns; current state: abandoned (direction change).
- [INT-0004](../../../intents/INT-0004-auditable-session-provenance.md) — revised; relevance: its constraint-provenance clause returns as a new requirement in INT-0011 AC-5 and INT-0012 AC-4; current state: abandoned.
- [INT-0005](../../../intents/INT-0005-safe-multilanguage-syntax-admission.md) — revised; relevance: syntax admission for Ferric's file tools, which are now in maintenance; current state: abandoned, with the shipped Python increment kept.
- [INT-0006](../../../intents/INT-0006-truthful-policy-contract.md) — revised; relevance: policy truthfulness now applies to the extracted core's exported contract; current state: superseded by INT-0011.
- [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md) — revised; relevance: calibration of the constrained core on the ~27B target moves to INT-0013; current state: superseded by INT-0013.
- [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) — revised; relevance: the human front door and local-model workflow, which Hermes now provides; current state: abandoned, with shipped increments kept.
- [INT-0009](../../../intents/INT-0009-lean-decomposed-architecture.md) — revised; relevance: the decomposition that now has a named consumer is core extraction; current state: superseded by INT-0011.
- [INT-0010](../../../intents/INT-0010-ferric-is-the-iron-of-amalgam.md) — created; relevance: charter for the new direction, the ownership split with Amalgam, and the maintenance freeze; current state: proposed; recommended for this sprint.
- [INT-0011](../../../intents/INT-0011-standalone-constrained-decoding-core.md) — created; relevance: extracting the constrained-decoding core; current state: proposed; recommended for this sprint (first increment).
- [INT-0012](../../../intents/INT-0012-constrained-valve-at-hermes-boundary.md) — created; relevance: the OpenAI-compatible valve Hermes points at, implementing Amalgam's unbuilt arm B; current state: proposed; next sprint.
- [INT-0013](../../../intents/INT-0013-constrained-decoding-on-midsize-quantized-target.md) — created; relevance: action-level evidence of whether grammar makes a ~27B Q4 model faster or more accurate; current state: proposed.
- [INT-0014](../../../intents/INT-0014-adaptive-constraint-policy.md) — created; relevance: adaptive constrained decoding (arm C) inside the valve, gated on Amalgam's arm-B result; current state: proposed.

## 1. Sprint Goal

The owner has redirected Ferric. Hermes Agent, forked as Animus Amalgam, is the
agent the owner wants to use. Ferric's job is to supply what Hermes lacks
entirely: harness-owned constrained decoding. This sprint resets Ferric's
intent set around that role and starts the work. It lands the charter
(INT-0010: documentation, instructions, ledger triage and a green `main`) and
the first increment of the standalone core (INT-0011). That increment moves
action-grammar authoring, parsing, control branches, protocol selection and
constrained-JSON stream scanning out of `ferric-loop` and `ferric-provider`
into a dependency-light crate, makes unsupported constraints fail explicitly,
and accepts OpenAI-format tools. The valve (INT-0012) and the qualification
work (INT-0013) build on that crate in later sprints.

## 2. Existing Code Survey

| File | Relevance | Notes |
|------|-----------|-------|
| `README.md`, `docs/README.md`, `docs/introduction.md` | high | Present Ferric as a standalone 1B–14B coding assistant. INT-0010 AC-1 retargets them. |
| `crates/ferric-loop/src/grammar.rs` | high | `action_schema` (one `anyOf` branch per tool, with required `thought`, `tool` and `args`), `parse_json_action`, `parse_action`. The core of the Iron, currently inside the loop crate. |
| `crates/ferric-loop/src/terminator.rs` | high | `task_complete`, `submit_plan` and `request_user_input` descriptors, which become the valve's final-answer and clarification branches. |
| `crates/ferric-loop/src/protocol.rs` | high | `select_protocol`: constraint, then native tools, then XML fallback, chosen from `Capabilities`. |
| `crates/ferric-loop/src/run.rs` | high | `step()` builds the `Constraint::JsonSchema` per turn, enforces ADR-010 exclusivity, refuses to parse a truncated action, and records `ConstraintApplied`. First consumer of the extracted core. |
| `crates/ferric-loop/src/{repetition,progress,failure,oscillation}.rs` | medium | The guard family. Pure logic, but keyed on loop types. Candidate for a later INT-0011 increment. |
| `crates/ferric-core/src/scale.rs` | high | `ModelProfile` → `RunPolicy`, tiers, `ring_for_tier`, `TierSource`. It already depends only on serde, serde_json and thiserror, but still exports inert `uses_planner`, plan budgets and `allows_subagents` (INT-0011 AC-7). |
| `crates/ferric-tools/src/registry.rs` | medium | Ring filtering and outer-ring-first trimming, tied to Ferric's builtin `ToolSpec.ring`. The core needs a caller-supplied ring map. |
| `crates/ferric-provider/src/types.rs` | high | `ToolDescriptor`, `Constraint {JsonSchema, Regex, Lark}`, `CompletionRequest::validate`, `Capabilities`. |
| `crates/ferric-provider/src/openai.rs` | high | `build_body` sends `response_format.json_schema` with `strict: true`. The `Regex` variant is accepted but sent unconstrained: the capability overclaim Amalgam found (INT-0011 AC-4). |
| `crates/ferric-provider/src/stream_scan.rs` | medium | `ConstrainedJsonScanner` extracts the early tool name, thought and summary deltas. The valve's streaming depends on it. |
| `crates/ferric-cli/src/startup/models.rs` | medium | Carried-over, uncommitted fix for the only failing test on `main` (Linux-only). Verified and committed as `bf3c7d8`. |
| `docs/work/tasks.md` | medium | 48 open tasks, mostly for the standalone surfaces. INT-0010 AC-3 triage. |
| `CLAUDE.md`, `AGENTS.md` | medium | Project instructions without the new direction (INT-0010 AC-5). |
| Amalgam `docs/lineage/animus-ferric.md` | high | External review of Ferric at `51af84e`: grammar buried in `ferric-loop`, regex overclaim, "not a decoder". |
| Amalgam `docs/lineage/architecture-comparison.md` and `hermes-decoding-seams.md` | high | Recommend a narrow Rust service at Hermes's custom endpoint that returns ordinary responses. Hermes owns the loop, tools and authority. |
| Amalgam `docs/lineage/local-evaluation-protocol.md` | high | Defines arm B ("static action constraint"), which is not implemented anywhere in Amalgam's lab. The valve fills it. |
| Amalgam `docs/lineage/direction-review.md` | high | 27B at about 3.6 tok/s (0.28 s/token). Decoded tokens dominate cost. "Constrained decoding pays off in fewer tokens." |
| Amalgam `docs/intents/INT-0004`, `INT-0005`, `INT-0007` | high | System-level qualification, the adaptive-policy definition and host-derived deadlines. Ferric's new intents defer to them. |

## 3. External Sources

- [llama.cpp server README](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md) — documents `response_format`/`json_schema`/`grammar`, `tool_choice`, `reasoning_format`/`reasoning_budget`, `chat_template_kwargs` and the per-response `timings` (prompt, predicted and cached token counts). It does not document how a JSON-schema grammar combines with native tools or with reasoning, or whether generation stops on client disconnect. Those gaps become explicit measurements in INT-0012 and INT-0013.
- [llama.cpp function-calling documentation](https://github.com/ggml-org/llama.cpp/blob/master/docs/function-calling.md) — covers native tool-call support per template family. It does not explain lazy versus forced grammar mechanics, which is why INT-0013 measures native arms directly instead of assuming "native" means unconstrained (matching Amalgam's arm-A labeling rule).
- [Hermes Agent (upstream)](https://github.com/NousResearch/hermes-agent) — the Mercury. Its custom OpenAI-compatible endpoint is the attachment point the valve targets.
- [Animus Amalgam Book at `5f6a0c7`](https://github.com/crussella0129/Animus_Amalgam/tree/5f6a0c7b60b297c4ee99ea01bba17cb63a9a8a5a/docs) — the sibling Book: lineage analysis, lessons L-01 to L-19, evaluation protocol and intents. The new Ferric intents pin their Amalgam links to this commit because Amalgam's local `dev` (Sprint 3, `f820dbf`) is not yet pushed.

## 4. Risks, Unknowns, Dependencies

- **Unknown:** How the pinned llama.cpp build applies a `json_schema` grammar to a reasoning model (Qwen3.8 thinking): reasoning first, suppressed, or broken. This is undocumented upstream; INT-0013 AC-4 measures it.
- **Unknown:** Whether per-request grammar changes are cache-neutral in practice. They should be, because grammars affect sampling and not prompt KV, but INT-0014 AC-2 has to demonstrate it with cached-token receipts before adaptive narrowing is trusted.
- **Unknown:** Whether the pinned build uses llguidance or llama.cpp's own JSON-schema-to-grammar converter. This affects supported schema constructs and compile cost for a Hermes-sized catalog. INT-0011 AC-3 records supported constructs; INT-0013 measures compile overhead.
- **Risk:** Hermes's tool catalog is large (Amalgam Sprint 2 found it "too large for the window"). An `anyOf` with dozens of branches, plus tool descriptions rendered into the prompt, may cost prompt tokens and grammar compile time. This is measured in INT-0013 AC-2; mitigation such as ring-limiting the grammar is INT-0014's job.
- **Risk:** Hermes tool schemas may use constructs the converter rejects. Hermes already runs a llama.cpp-specific sanitizer (Amalgam seams chapter, `model_tools.py:515`); the valve should consume its output and reject anything else with a typed error, never pass it through.
- **Risk:** Extracting `ToolDescriptor`, the terminators and the scanner touches import paths across `ferric-loop`, `ferric-provider`, `ferric-bench` and `ferric-cli`. It is mechanical, but the whole suite must stay green (INT-0011 AC-2). A fix and a refactor must not share a commit.
- **Risk:** Freezing the standalone surfaces could let them rot. Mitigation: INT-0010 AC-6 keeps CI green, and red Dependabot PRs are handled at sprint boundaries under the dependency-intake rule. The open ones were red because of the test fixed in `bf3c7d8`.
- **Dependency:** Live readiness (INT-0012 AC-7) and qualification (INT-0013) need Amalgam's isolated lab, the GPU host and the Qwen3.8-27B Q4 artifact already on disk. No download is needed or authorized.
- **Dependency:** Cross-repository pinning. Amalgam cites Ferric at `51af84e`, and Ferric now cites Amalgam at `5f6a0c7`. INT-0010 AC-4's versioned release lets both Books pin each other deliberately.
- **Carry-over resolved:** `main` CI had been red since the Sprint 125 merge because `startup::models::tests::discovered_models_directory_swap_cannot_admit_external_model` failed on Linux only. The uncommitted fix passed 7/7 on Windows and in WSL Ubuntu ([artifact](linux-models-fix-verification.txt)) and landed as `bf3c7d8`, so this sprint's PR restores green.

## 5. Recommended Approach

Primary: two work packages, planned for one sprint.

1. **Charter (INT-0010):** retarget `README.md`, `docs/README.md` and
   `docs/introduction.md` to the Iron role and the new model envelope, keeping
   shipped-behavior descriptions truthful. Add the direction to `CLAUDE.md` and
   `AGENTS.md`. Triage `docs/work/tasks.md` into Iron work and a labeled
   maintenance section without deleting anything. Confirm CI on `dev` is green
   on both platforms before the PR.
2. **Core, first increment (INT-0011):** create the dependency-light core
   crate. Move `ToolDescriptor`, `Constraint`, `Capabilities`, action-schema
   authoring, both parsers, the control-branch descriptors, protocol selection
   and `ConstrainedJsonScanner` into it, with an OpenAI-`tools` input adapter.
   Make an unenforceable constraint an explicit error (fixing the `Regex`
   overclaim). Pin the dependency boundary with a test. Rewire `ferric-loop`,
   `ferric-provider`, `ferric-bench` and `ferric-cli` to the crate with
   behavior unchanged. The policy, ring-map, guard and conformance-corpus
   increments (AC-5 to AC-8) follow in the next sprint, together with the
   valve.

Alternative considered: a "reverse-E2E" sprint that builds a minimal valve
against a real `llama-server` and Hermes first, extracting only what the valve
needs. That surfaces Hermes integration issues (streaming shapes, stale-stream
timeouts, cancellation) sooner and matches how the owner prefers to work in
Amalgam. It is not primary because a valve built before extraction would have
to depend on `ferric-loop`, or copy its code, and INT-0011 rejects both.
Extraction is small (about 1,100 lines of grammar, protocol, terminator and
scanner code, tests included) and mostly mechanical. If the owner prefers the E2E-first order,
the plan can swap the second work package for "extract grammar and parsers
only, plus a non-streaming valve smoke".

Rationale: the charter prevents future sessions from continuing standalone
work, such as INT-0008's acquisition backlog or `server.rs` splits, that no
longer serves the product. The extracted core is the precondition that every
later Iron intent shares.

## Artifacts
- `linux-models-fix-verification.txt` — WSL Ubuntu run of `startup::models` tests (7/7) verifying the carried-over Linux CI fix before commit `bf3c7d8`.

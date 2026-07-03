# Sprint 34 Research Report — Ornstein: the CaMeL-lite sink-policy primitive

## Sprint goal (in my words)
Designed with the user. Build the **CaMeL-lite flow-control primitive** — taint tracking + a
configurable **sink policy** — that decides whether a tool call whose args derive from tainted
(untrusted research) data may reach a side-effecting **sink**. **Pure primitive only this
sprint** (no loop wiring); **all three enforcement modes** selectable by the caller (Deny /
RequireApproval / Warn). Lives in `ferric-research`; fully unit-tested.

## The design (settled with the user)
- **Taint** (CaMeL-lite, substring tracking): a `ResearchDigest`'s text (summary + claim quotes)
  is tainted. A `TaintSet` holds tainted strings; a value is *tainted-derived* if it contains any
  non-empty tainted substring; a tool call's args are tainted if any string within the args JSON
  is tainted-derived.
- **Sink policy**, keyed off the existing `PermissionLevel { Read, Write, Execute }`:
  - not tainted → `Allow` (unchanged); `Read` + tainted → `Allow` (reading isn't a dangerous
    sink; the workspace boundary confines it); `Write`/`Execute` + tainted → the configured action.
  - `SinkAction { Deny, RequireApproval, Warn }` (the user's "all 3, caller picks"); `decide(...)
    -> SinkDecision { Allow, Deny, RequireApproval, Warn }`.
- **Enforcement point (deferred):** the eventual wiring sits at the dispatch chokepoint
  (`registry.execute`, beside the existing `check(permission, path)`), populating the `TaintSet`
  when digests enter the agent's context. This sprint builds the primitive; the wiring lands with
  the research→loop integration.

## Decisions Reviewed
- **ADR-040–043** — the quarantine + planes + orchestrator; digests are `untrusted` (the taint
  source). CaMeL is the flow-control on top. **ADR-005 / the guard model** — `PermissionLevel` +
  `Decision` + the `registry.execute` chokepoint is exactly where the eventual sink gate slots in,
  as a second check beside the permission check. No revision; additive.

## Existing Code Survey
| File | Role / relevance |
|---|---|
| `crates/ferric-guard/src/checker.rs` | `PermissionLevel { Read, Write, Execute }`, `Decision { Allow, Deny(DenyReason) }`, `check(level, path)`. The sink policy keys off `PermissionLevel` (Read = not-a-sink; Write/Execute = sink) and the eventual wiring mirrors `check`'s role at the chokepoint. |
| `crates/ferric-tools/src/registry.rs` (`execute`) | The chokepoint that already calls `check(spec.permission, path)` before `run`; the sink gate will sit beside it (deferred). |
| `crates/ferric-research/src/lib.rs` | `ResearchDigest { source, untrusted, summary, claims:[{claim,quote}] }` — the taint source; the new `sink` module lands here. |

## External Sources
- DeepMind **CaMeL** (dual-LLM + information-flow control) + Willison's lethal trifecta — recovered
  from the s1 research; "CaMeL-lite = tainted-string tracking + a sink-policy table, no
  interpreter" is the scoping the s1 doc set. No new fetch.

## Risks / unknowns / dependencies
- **Substring taint is conservative** (possible false positives: a benign arg that happens to
  contain a tainted substring gets flagged). Accepted for CaMeL-lite — erring toward over-gating a
  *write* is the safe direction; the modes (Warn/RequireApproval) give softer options than Deny.
- **The primitive is inert until wired** — it's a pure decision function this sprint; the value is
  a tested, ready-to-wire policy (the quarantine/retriever "primitive first" pattern). The wiring
  (taint set populated as digests enter context; gate at dispatch) is the next increment.
- **New dep:** `ferric-research` adds `ferric-guard` (workspace crate) for `PermissionLevel`. No
  cycle (`ferric-guard` doesn't depend on `ferric-research`).

## Recommended approach
A new `crates/ferric-research/src/sink.rs`:
- `enum SinkAction { Deny, RequireApproval, Warn }`; `enum SinkDecision { Allow, Deny,
  RequireApproval, Warn }`; `struct SinkPolicy { tainted_sink: SinkAction }` with `new(action)` +
  `deny()` convenience + `decide(&self, permission: PermissionLevel, tainted: bool) -> SinkDecision`.
- `struct TaintSet { tainted: Vec<String> }` with `taint_digest(&ResearchDigest)` (summary +
  each `claim.quote`), `taint_str(&str)`, `is_tainted(&str) -> bool` (contains any non-empty
  tainted substring), `args_tainted(&serde_json::Value) -> bool` (recursively walk the args JSON,
  any string tainted-derived).
- Re-export from `lib.rs`. Add `ferric-guard` dep.
- **Tests:** the `decide` matrix (not-tainted→Allow any level; Read+tainted→Allow; Write/Execute+
  tainted → each of Deny/RequireApproval/Warn per mode); `TaintSet` (taint_digest marks summary +
  quotes; `is_tainted` substring hit/miss; `args_tainted` walks nested object/array JSON; empty set
  taints nothing).

### Alternative considered — bake one enforcement mode in (just Deny)
Rejected per the user: support all three modes (the caller picks at wiring time) so Deny (autonomous),
RequireApproval (human-gated), and Warn (observability-first rollout) are all available without a
later breaking change.

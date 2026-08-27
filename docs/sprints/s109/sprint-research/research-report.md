# Sprint 109 research — what Codex's design says about Ferric's

Source: <https://github.com/openai/codex> (Apache-2.0, ~101.7k stars), read
2026-07-27. Primary sources throughout — crate listings and Rust source via the
GitHub API, plus the hosted docs. Where I could not verify a claim I say so.

## What Codex is, structurally

| | Codex | Ferric |
|---|---|---|
| Rust crates | **97** | **15** |
| Build | Bazel + pnpm monorepo | plain cargo workspace |
| Target model | frontier (GPT-5-class) | local small models, 1B–8B |
| Containment | **OS sandbox** | **in-process guard** |
| Decoding | native tool calls | **constrained JSON (grammar)** |
| Capability tiers | none found | `RunPolicy` / `measured_level` |

The 97-vs-15 gap is mostly surface area Ferric does not have (`tui`, `cloud-*`,
`app-server-*`, `chatgpt`, `login`, `keyring-store`, `analytics`, `otel`,
`v8-poc`), not deeper factoring of the same problem. It is not evidence Ferric
is under-decomposed.

## 1. Sandboxing — the biggest single divergence

Codex enforces containment **at the OS boundary**: macOS Seatbelt, Linux/WSL2
`bubblewrap` (user-namespace isolation, with a bundled fallback), and native
Windows Sandbox. Three modes — `read-only`, `workspace-write` (default),
`danger-full-access` — crossed with three approval policies — `untrusted`,
`on-request`, `never`. Network is **denied by default**; in managed
deployments it is allowlisted by hostname.

Ferric enforces containment **in-process**, in `ferric-guard`, at the single
registry chokepoint. Its OS-level sandbox (the Docker airlock, ADR-081/083/085)
exists only for Ornstein's research plane, not for ordinary tool dispatch.

**This is the finding that reframes several others.** Codex can afford a
permissive default posture *because the OS is holding the line underneath it*.
Ferric has no such floor for normal runs, so every relaxation Ferric makes is
load-bearing in a way the equivalent Codex relaxation is not. Copying Codex's
defaults without copying Seatbelt/bubblewrap would import the looseness and
leave the backstop behind.

## 2. `execpolicy` vs `ferric-guard::check_command` — a real, concrete gap

Codex's `execpolicy` crate is a **declarative Starlark policy engine**:

```starlark
prefix_rule(pattern = ["git", "status"], decision = "allow")
prefix_rule(pattern = ["rm", "-rf"], decision = "prompt",
            justification = "Recursive deletion requires confirmation")
prefix_rule(pattern = ["curl"], decision = "forbidden",
            justification = "Use `wget` for downloads instead")
host_executable(name = "git", paths = ["/usr/bin/git", "/opt/homebrew/bin/git"])
```

Properties: **argv-token prefix matching** (not string containment), a
**three-valued decision** (allow / prompt / forbidden), **strictest-severity
wins** across matching rules, a **`justification`** that names an alternative,
and **`host_executable()`** pinning which absolute paths may satisfy a
basename match.

Ferric's equivalent, in full (`checker.rs:130`):

```rust
pub fn check_command(command: &str) -> Decision {
    let lowered = command.to_ascii_lowercase();
    for pattern in DENIED_COMMAND_PATTERNS {   // ["rm -rf /", "mkfs", "dd if=",
        if lowered.contains(pattern) {         //  "git push --force",
            return Decision::Deny(...)         //  "shutdown", "reboot"]
```

Three differences that matter, in descending order of substance:

1. **No path pinning.** Ferric matches text, so it has no notion of *which*
   `git` ran. `host_executable()` is a defence Ferric has no analogue for.
2. **Substring, not tokens.** `contains("rm -rf /")` is a spelling test, not a
   semantic one — a speed bump against an obvious mistake, not a boundary.
   This is *arguably fine*: Ferric's real containment is the path guard at the
   registry chokepoint. But the denylist should be **described** as a
   footgun-catcher rather than a security boundary, because nothing currently
   draws that line and its presence invites the opposite reading.
3. **Two-valued, not three.** Ferric already has the three-valued shape —
   `SinkAction::{Deny, RequireApproval, Warn}` (ADR-080/081) — but only on the
   *sink* path. `check_command` cannot say "ask the human". The vocabulary
   exists; it just is not wired here.

**Independent convergence worth noting:** allow/prompt/forbidden and
`SinkAction`, and Codex's strictest-wins layering and `.ferricignore`'s
additive-only rule (ADR-068), are the same conclusions reached separately.

## 3. Skills — this answers the deferred `allowed-tools` question

From `codex-rs/skills/src/model.rs`:

```rust
pub struct SkillMetadata {
    pub name: String, pub description: String,
    pub interface: Option<SkillInterface>,
    pub dependencies: Option<SkillDependencies>,
    pub policy: Option<SkillPolicy>, ... }

pub struct SkillPolicy {
    pub allow_implicit_invocation: Option<bool>,
    pub products: Vec<Product>, }

pub struct SkillDependencies { pub tools: Vec<SkillToolDependency> }

pub struct SkillToolDependency {
    pub r#type: String, pub value: String, pub description: Option<String>,
    pub transport: Option<String>, pub command: Option<String>,
    pub url: Option<String>, }
```

Three deductions:

**(a) `SkillDependencies.tools` is a declaration of *need*, not a grant of
*permission*.** The fields — `transport`, `command`, `url` — are MCP server
coordinates. The skill says *"I expect this tool to exist"*, so the host can
check the precondition and tell the user what is missing. Nothing in the type
widens what the agent may do.

That is a clean resolution to the question ADR-091 deferred. Ferric can honour
an `allowed-tools` frontmatter key as a **precondition check** — refuse to
compose a skill whose declared tools fall outside the active rings, and say
which — **without it ever becoming an action channel**. The key stops being a
security decision and becomes a diagnostic.

**(b) Codex defaults to model-invocable skills.**
`allows_implicit_invocation()` returns `true` when unset. Ferric's `Authority`
has **no `Model` variant at all** (ADR-091). This is the sharpest divergence in
the two designs, and §1 explains it: Codex's blast radius on a bad implicit
invocation is bounded by Seatbelt; Ferric's would be bounded only by the guard.
**Ferric's stricter default is right for Ferric**, and should not be relaxed by
pointing at Codex.

**(c) Their own code records the trap this project keeps finding.** Verbatim
from `model.rs`:

> `TODO: Enforce product gating in Codex skill selection/injection instead of only parsing and storing this metadata.`

A policy field that is parsed, stored, surfaced — and enforced nowhere. That is
ADR-080's `TaintSet` (a detector that did not detect) and ADR-093's `trace
verify` (a drift detector that always said drift), in a codebase with 101k
stars. Independent evidence the failure mode is structural, not local
sloppiness.

## 4. No constrained decoding — and why that is not an argument against Ferric

Searching the repository for grammar/constrained decoding returns 12 Rust hits,
**all of them parsers**: `apply-patch/src/parser.rs`, `shell-command/src/bash.rs`,
`utils/path-uri`, `tui/render/highlight.rs`, plus two tool *specs*. There is no
decoding constraint anywhere; Codex relies on the frontier model's native
tool-calling.

Ferric's founding thesis (ADR-010/015/022) is the opposite, and it is measured:
the constrained path holds at **100% down to a 1B model** where the same model's
native tool-calling collapses to 22%. Codex does not need that because it never
talks to a 1B model.

**Consequence:** Codex's prompt shapes, tool schemas and turn structure are
tuned for a model that does not need help. They are the wrong thing to copy
wholesale, and Codex's absence of tiering is not evidence Ferric's tier system
is over-built — it is evidence the two target different hardware.

## 5. `code-mode` — noted, and deliberately not adopted

`codex-rs/code-mode` embeds **V8** (`v8_init.rs`, `V8JitMode`,
`initialize_v8`): the model writes JavaScript that composes tool calls, instead
of emitting one action per turn. Fewer round-trips, real control flow.

Against Ferric: it is the direct antithesis of the constrained
one-action-per-turn valve, it presumes a model that writes correct JS unaided,
and it puts a JS engine inside a Rust harness whose stated baseline is
Raspberry-Pi-class aarch64. Recorded as a known alternative, not a candidate.

## What I could not verify

The repo's `docs/config.md` and `docs/example-config.md` are now redirect stubs,
and `learn.chatgpt.com/docs/skills` 404s. So the **frontmatter key names** a
`SKILL.md` author actually writes are inferred from the Rust types, not read
from a published schema. The Rust model is authoritative for *structure*; the
exact YAML spellings are not confirmed, and any Ferric work that claims
"conventional format" compatibility needs them checked first.

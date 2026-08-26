# Sprint 41 Research — chat mode + the containerization pivot

## Decisions Reviewed
- **ADR-005** (security is hardcoded and harness-owned): whatever chat mode ships, the LLM is
  never consulted on a security decision — this applies identically inside or outside a container.
- **ADR-010** (constraint/tools mutual exclusivity per request): chat mode's own action framing (if
  any) must still satisfy this — a "genuinely conversational" surface cannot silently reintroduce
  unconstrained tool dispatch.
- **ADR-011** (no chat catch-all, revised 2026-06-29): the ORIGINAL decision rejected a REPL/chat
  mode entirely. The revision (recorded in memory, not yet in `decisions.md` as its own ADR number)
  approved building it, split into two pieces: `ferric mcp` (shipped, ADR-046) and a genuine raw
  chat mode (still unbuilt) — explicitly requiring **its own dedicated ADR on the chat security
  boundary**, not a quiet amendment.
- **ADR-012** (OpenClaw integration is MCP-stdio-first): unaffected by this sprint; chat mode is a
  human-facing surface, not an agent-integration one.
- **ADR-046** (`ferric mcp`'s launch-time-fixed containment): the closest existing precedent for
  "a new entrypoint, not a new decoding path or a new privilege." Its closing line explicitly
  deferred chat mode to "its own future sprint + own dedicated ADR" — this is that sprint.
- **ADR-014** (capability roadmap): originally sequenced "Docker/Nix capability layer" for s3
  (never built — Ornstein's container/proxy increment has carried this forward, gated on a
  containerizer that still isn't installed).
- **Ornstein's own deferred container increment** (`docs/ornstein.md`, ADR-040/044): the hardened
  container + allowlist egress proxy for the Web retriever plane has been "gated on a containerizer
  — none installed yet" since sprint 30. This sprint's platform-wide containerization question is a
  generalization of that same, still-unresolved blocker — not a new one.

## Sprint goal (own words)
The user picked chat mode (the ADR-011 revision's still-unbuilt half) from a shortlist, then
immediately reframed it inside a much larger question: **should the whole Animus platform run
inside containers** (Docker preferred, k8s/n8n considered, chosen for flexibility from one machine
to a whole data center), with chat mode, the Ornstein search subsystem (double-containered so "the
airlock model" lives in one of the layers), and possibly MCP each in their own nested containers —
citing Docker-in-Docker (DinD) as the candidate mechanism. This report investigates both: chat
mode's own design space, and whether DinD is actually the right building block for what the user
described.

## Existing Code Survey
| File | Relevance |
| --- | --- |
| `crates/ferric-cli/src/main.rs` | The `Command` enum (`Query`/`Bench`/`Toolbench`/`Mcp`/`Server`/`Trace`) — a new `Chat` variant would slot in identically to how `Mcp` did in ADR-046. Module doc comment already names chat mode as the still-unbuilt ADR-011-revision half. |
| `crates/ferric-cli/src/mcp.rs` | ADR-046's shipped precedent: launch-time-fixed containment (workspace/backend/model pinned as CLI flags, not per-call), errors-never-crash-the-server, one exposed action. The most direct template for chat mode's OWN security-boundary design, adapted for multi-turn conversation instead of one MCP tool call. |
| `crates/ferric-loop/src/run.rs` | `run()`'s turn loop always frames actions through `ActionProtocol` (constrained JSON / native tools / TextXml) — there is no existing "raw, unconstrained" completion path anywhere in the loop. A genuine chat mode's "what's constrained vs. not" design question (per the ADR-011 revision's own framing) has to answer: does a chat turn still route through this SAME dispatch/guard chokepoint (harness still owns decoding, just with a conversational UX on top), or does it bypass it (a real security-boundary expansion, needing the dedicated ADR)? |
| `crates/ferric-guard/src/checker.rs` | The workspace-containment/deny-list enforcement (ADR-005) — whatever chat mode does to the filesystem must still pass through this, regardless of container boundary. Containerization is a DIFFERENT, complementary isolation layer (process/kernel), not a replacement for this one (in-process permission logic). |
| `crates/ferric-research/src/lib.rs` + `docs/ornstein.md` | Ornstein's quarantine is today a STRUCTURAL guarantee (constrained decoding + empty tools), not a container boundary. The "double-container the search container so the airlock model lives in one of the layers" idea would ADD a process/kernel isolation layer underneath the existing structural one — complementary, not a replacement. The Web retriever + hardened container + allowlist proxy increment has been blocked on a containerizer since sprint 30 (ADR-040/044) — this sprint's platform question generalizes, not replaces, that blocker. |
| `Cargo.toml` (workspace root) | No container-orchestration or sandboxing crate (bollard, testcontainers, etc.) is in the dependency graph yet — confirms containerization work would be greenfield, not an extension of existing plumbing. |
| `agent-tasks/agent-tasks.md` | The "Web retriever + container" backlog item already carries the exact blocker text ("docker/podman absent on Windows+WSL; I can't install it... the USER must install") — unchanged as of this sprint. |

(7 files/table rows — within the 20-file research budget; no override needed.)

## External Sources
The user's own framing named Docker-in-Docker (DinD) specifically, citing a 2023 DockerCon article
on CI workflows. Current (2026) practice has moved substantially since then for the SPECIFIC use
case Ornstein's "airlock" is meant to solve (isolating execution of untrusted/agent-touched
content) — worth checking before building on the cited pattern:
- [Running AI Coding Agents in Docker Sandboxes: Why Containers Are No Longer Enough](https://xplicit.medium.com/running-ai-coding-agents-in-docker-sandboxes-why-containers-are-no-longer-enough-76e0f65f2ce5) (Medium, 2026) —
  plain Docker-in-Docker (mounting `docker.sock`, or nesting a full `dockerd`) is now treated as a
  real anti-pattern for untrusted workloads: "operationally equivalent to full root on the host
  with extra steps." Names three 2024–2026 CVEs (runc `CVE-2024-21626`, NVIDIA Container Toolkit
  `CVE-2025-23359`, kernel io_uring `CVE-2026-1109`) that each broke the container/host boundary —
  containers sharing the host kernel means one successful exploit from inside is root outside.
- [Docker Sandboxes: Run Claude Code and More Safely](https://www.docker.com/blog/docker-sandboxes-run-claude-code-and-other-coding-agents-unsupervised-but-safely/) (Docker's own blog) —
  Docker's own answer to exactly this problem, GA'd January 2026: each agent workload runs in a
  dedicated **microVM** (hypervisor boundary, not shared-kernel namespaces), so agents can build
  and run Docker containers INSIDE the sandbox with **no access to the host Docker daemon** — this
  is real DinD-shaped functionality, but built on a VM boundary rather than nested dockerd. Runs on
  **macOS and Windows today** (Linux "planned"); Windows support presumably rides on Docker
  Desktop's existing WSL2/Hyper-V backend, though the exact mechanism isn't documented in the
  fetched page.
- Corroborating overview material (not individually fetched, cited from search-result summaries):
  the 2026 sandbox-isolation hierarchy is widely described as Level 1 (shared-kernel containers —
  Docker/Podman, fast but kernel-vulnerability-exposed), Level 2 (userspace-kernel interception —
  gVisor), Level 3 (microVMs — Firecracker, Kata Containers, libkrun, hardware-virtualized). For
  workloads executing untrusted or AI-generated code specifically, 2026 industry consensus treats
  Level 1 alone as insufficient and converges on Level 3 (Fly.io, E2B, Unikraft, AWS Lambda, Kata
  Containers, and now Docker Sandboxes itself all use microVM isolation for this exact use case).

**Key correction to the user's framing:** "Docker-in-Docker" as literally described (nesting a
second Docker daemon inside a container, the pattern the cited 2023 CI-workflow article covers) is
the WRONG tool for the "airlock" half of what was described — it adds operational complexity
without the isolation guarantee the use case actually needs, and current practice actively warns
against it for untrusted-content execution. It may still be the RIGHT tool for a narrower, different
problem (a CI-style container that needs to build/run OTHER containers as part of its own job) — but
that's not what Ornstein's search-container airlock is for.

## Risks, unknowns, dependencies
1. **The Windows Docker/podman blocker is unchanged and now platform-wide, not just Ornstein-
   scoped.** No admin access; WSL sudo needs a password neither available in this session. WSL2
   itself IS present (`wsl -l -v` shows Ubuntu, version 2, stopped) — a prerequisite Docker Desktop
   needs, but installing Docker Desktop itself still requires elevation. This blocks ANY live
   container testing (build, run, compose) regardless of which architecture is chosen. Confirmed
   fresh this sprint (`docker --version`/`podman --version` both "command not found").
2. **Two genuinely different problems got named as one ("containerize everything" +
   "double-container the airlock"):** (a) platform-wide DEPLOYMENT flexibility (single machine to
   datacenter — a service-topology question, naturally answered by ordinary sibling containers via
   Docker Compose, scaling to Kubernetes if datacenter-scale is ever real) and (b) SECURITY
   isolation for untrusted-content execution specifically (Ornstein's airlock — naturally answered
   by a microVM-class sandbox, not nested Docker). Conflating them risks either over-engineering
   (b)'s heavy isolation onto every service, or under-isolating (b) by settling for plain sibling
   containers where a kernel-shared boundary isn't enough.
3. **Chat mode's own security-boundary design is a substantial, still-open question independent of
   containerization.** ADR-046's precedent (launch-time-fixed containment, one narrow exposed
   action) doesn't automatically answer "what can a multi-turn conversational surface do that a
   single `ferric_query` MCP call can't" — this needs its own careful design, matching the ADR-011
   revision's explicit requirement for a dedicated security-boundary ADR, not a quiet amendment.
4. **Scope risk: this sprint could easily balloon into "design the entire platform" without
   shipping anything concrete**, mirroring what sprint 35 (an audit-only sprint) deliberately chose
   to be. Chat mode itself has NO container dependency — it's an in-process Rust CLI surface,
   buildable today exactly the way `ferric mcp` was (ADR-046 shipped with zero container
   involvement). The containerization architecture question is real and worth deciding, but
   deciding it doesn't require Docker to be installed (a design/ADR + Dockerfile/compose skeletons
   can be written and reviewed without ever running `docker build`).

## Recommended approach (+ alternative considered)
**Recommended: split this sprint's output, mirroring the sprint 39→40 precedent.**
- **Ship this sprint:** chat mode's own design + build — an in-process Rust CLI surface (`ferric
  chat`), following ADR-046's launch-time-fixed-containment template, with its own dedicated ADR
  answering the "what's constrained vs. not" question the ADR-011 revision requires. No container
  dependency; buildable and testable today (matches `ferric mcp`'s validated precedent).
- **Also ship this sprint (design only, no live testing — Docker still isn't installed): a
  container-architecture ADR** correcting the DinD framing: sibling containers (Docker Compose,
  k8s-ready) for platform-WIDE deployment flexibility, vs. a microVM-class sandbox (Docker
  Sandboxes, or gVisor if a non-Docker-specific option is preferred) for Ornstein's airlock
  specifically — these are two different tools for two different problems, not one nested-DinD
  answer to both. Skeleton Dockerfiles/compose files can be committed now (syntax-checkable without
  a running daemon) as the concrete starting point for whenever Docker access is resolved.
- **Explicitly defer:** actually building/running any container (still blocked on the
  admin-access-gated Docker install); MCP's own containerization question ("we can assess" — the
  user's own words signal this isn't decided yet, and nothing forces a decision before the
  architecture ADR exists to decide it against).

**Alternative considered: build the full nested-container platform architecture literally as
described (DinD, chat+Ornstein+MCP each in their own nested layer) before touching chat mode's own
code.** Rejected as this sprint's PRIMARY scope: it's blocked end-to-end on the same unresolved
Docker-install blocker that's stalled Ornstein's web-retriever increment since sprint 30, would
produce no testable/mergeable code this sprint, and — per the external research above — would
likely need to be redesigned anyway once DinD's known problems for the airlock use case are
factored in. The corrected architecture (sibling containers + a purpose-built sandbox for the
airlock) is a better target to design toward even once Docker access unblocks.

## Scope Decided (user, 2026-07-09, after research)
Presented with the recommended split (chat mode built now + a design-only container ADR) as one of
four options via `AskUserQuestion`, the user chose: **"Container architecture only this sprint"**
— focus entirely on the container-architecture ADR + design this sprint; chat mode's own build is
explicitly deferred to sprint 42 (not bundled here, not dropped). Locked scope for the Plan Phase:
- **In scope:** the container-architecture ADR (sibling containers via Docker Compose for
  platform-wide deployment flexibility, one machine to a datacenter, k8s-ready; a microVM-class
  sandbox — Docker Sandboxes or gVisor — for Ornstein's airlock specifically, correcting the DinD
  framing); skeleton Dockerfiles/compose files as the concrete starting artifact (syntax-checkable
  without a running daemon, since Docker still isn't installed); where chat/Ornstein/MCP each fit
  in the corrected topology, and explicit "we can assess" language preserved for MCP's own
  containerization question (not decided this sprint, no need to force it).
- **Out of scope, explicitly deferred:** chat mode's own code + its dedicated security-boundary ADR
  (sprint 42); any live container build/run/test (still blocked on Docker installation, admin
  access); MCP containerization decision (deferred pending the user's own future call).

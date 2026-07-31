# The Animus Project

Animus Ferric is one component of a larger whole. **[The Animus
Project](https://github.com/crussella0129/The_Animus_Project)** describes itself
as *"an open-source ecosystem of accessible and highly performant local AI
applications"* — model harnesses provide the AI infrastructure, while a set of
tools and agent skills deliver specialized functionality on top.

This appendix summarizes the sibling components and how Ferric relates to each.
Status labels (*Current* / *Under Development* / *Deprecated*) are as listed on
the project's own overview; follow the project link above for the authoritative,
up-to-date roster.

## Model harnesses

The harness runs a local model. Ferric is the current one; the others are its
lineage.

- **Animus Ferric** *(Current)* — this project. The active harness: harness-owned
  decoding for small local models.
- **Animus Fev** *(Deprecated)* — a previous harness iteration.
- **Animus Prion** *(Deprecated)* — an earlier harness version, no longer
  maintained.
- **Animus 1.0** *(Deprecated)* — the original harness foundation.

## Tools & agent skills

These sit alongside the harness. Several connect to Ferric directly:

- **Animus Sprint-Loops** *(Current)* — the workflow-management and agent-
  coordination system. The five-phase sprint loop (Research → Plan → Build → Test
  → Loop) this project is developed under.
- **Animus Dark Matter — Local Intelligence Multiplier** *(Under Development)* —
  a knowledge layer that serves a task *just* the context it needs. Ferric already
  implements its side of the seam: the [`fetch_reference`](advanced-mcp.md) tool in
  [ICM](icm.md), whose in-tree backend is a stand-in for Dark Matter's future MCP
  knowledge server.
- **Animus AutoResearch** *(Current)* — automated research capabilities;
  conceptually adjacent to Ferric's own [Ornstein](ornstein.md) quarantined-
  research subsystem.
- **Animus GECK** *(Current)* — a toolset component. Ferric's `ferric launch`
  bootstrapper is the in-harness descendant of this scaffolding lineage.
- **Animus IDE** *(Under Development)* — an integrated development environment; the
  intended graphical organ that would drive Ferric's `chat` and `mcp` surfaces.
- **Animus Model Lab** *(Under Development)* — a testing environment for model
  experimentation (fine-tunes, conversions, evaluation).
- **Animus Neutronium** *(Current)* — an active operational component (front-end /
  UI-layer engineering).
- **Animus Puccinia** *(Under Development)* — an emerging system module.

## How Ferric fits

Ferric is the **spine** ([Why Animus?](introduction.md)) — the harness that turns a
small model's intent into reliable, contained action. The other components extend
it outward: Dark Matter feeds it sharper knowledge, AutoResearch and Ornstein feed
it the outside world under quarantine, Sprint-Loops governs how it is built, GECK
and Launch bootstrap new work, and the IDE will eventually give it a face. Animus
Ophanim (see [The Inference Server](server-configuration.md)) is the intended
native engine that will one day replace the external llama.cpp backend Ferric
drives today.

> Component names, status, and scope evolve. Treat
> [The Animus Project overview](https://github.com/crussella0129/The_Animus_Project)
> as the source of truth; this appendix is a snapshot with Ferric's perspective
> added.

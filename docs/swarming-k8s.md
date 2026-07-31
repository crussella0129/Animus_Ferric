# Container Topology & Roadmap

> **This chapter is mostly stated intent.** The single-node container topology
> exists and is described first as current fact. Multi-node **swarming with
> Kubernetes** is *not built yet* — it is recorded here as direction, so a future
> sprint reads this page to know what it was asked to build. Where a line
> describes something shipped, it says so.

## Two problems, two tools

Container work in Animus was shaped by one correction (ADR-051): "run it in
containers" conflates two unrelated problems, and they need different tools.

1. **Deployment flexibility** — running the platform anywhere from one laptop to a
   datacenter. Solved with ordinary **sibling containers** (Docker Compose today,
   Kubernetes-ready later). This is the subject of this chapter.
2. **Untrusted-content isolation** — sandboxing what [Ornstein](ornstein.md)
   fetches. Solved with a **microVM-class sandbox** (gVisor / Docker Sandboxes),
   *not* nested Docker. That is a separate concern, covered under Ornstein's
   airlock — do not conflate it with the topology here.

Literal Docker-in-Docker was explicitly rejected for isolation: a shared-kernel
boundary is the wrong tool for untrusted content, and mounting the docker socket
into the container that handles that content stages a host-root escape.

## What exists today: sibling containers *(shipped)*

`docker/docker-compose.yml` is a single-node topology skeleton. One service is
real and buildable:

- **`ferric-core`** — the harness and its co-located inference backend
  (`llama-server`) in **one image** (`docker/Dockerfile`, built with
  `--features backend-openai`). They are co-located deliberately: ADR-005 pins
  their link to loopback, so splitting them across a container boundary would break
  that guarantee without extra network-namespace plumbing. The container mounts a
  `/workspace` and the shared Animus model store at `/models`.

Other services (a raw `chat` surface, and any future Ornstein service) are present
only as **commented stubs** marking where later sprints land. There is no docker
socket mount and no host-published port: when sibling services eventually need to
reach `ferric-core`, the intended path is an **internal** Docker network, not a
published port — a decision left to the sprint that operationalizes them.

```sh
# Build and inspect the current topology:
docker compose -f docker/docker-compose.yml build ferric-core
docker compose -f docker/docker-compose.yml config
```

The model store path is overridable with `MODELS_PATH` in a local, gitignored
`docker/.env` (see `docker/.env.example`).

## Where this is going: swarming with Kubernetes *(intent)*

The compose topology above is deliberately **k8s-ready** but the Kubernetes layer
itself is not written. The stated intent, to be built out in a future sprint:

- **A Ferric worker as a schedulable unit.** Package `ferric-core` (harness +
  co-located backend, loopback pin intact) as a pod that Kubernetes can schedule,
  replicate, and place on nodes with the right hardware (GPU nodes for larger
  models, CPU/edge nodes for `Nano`/`Small` tiers).
- **A swarm of small models over one big one.** This is the thesis at cluster
  scale: rather than one large model, a *fleet* of small, tier-calibrated Ferric
  workers, each provably driving its tools, dispatched to tasks matched to its
  measured tier. The [tier/ring](advanced-tool-rings.md) model is what makes a
  heterogeneous swarm meaningful — every worker's capability is a measured number,
  not a guess.
- **Placement by measured capability.** `model_profiles.json` already records a
  model's earned tier; the intent is to make that the input to scheduling, so work
  routes to the smallest worker that can complete it.
- **Isolation preserved across the swarm.** Ornstein's airlock stays a
  per-run microVM concern on whichever node runs it; the swarm layer handles
  deployment and scheduling, never untrusted-content isolation.

None of the above is implemented. It is written here so the design constraints
that already exist — the loopback pin, the one-image co-location, the
measured-tier model — are on record for the sprint that builds the swarm, and so
that intent does not evaporate between now and then.

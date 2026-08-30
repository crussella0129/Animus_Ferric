# The Inference Server

Animus does not contain a model runner. It drives an **OpenAI-compatible HTTP
server** — the "valve" — and `ferric server` is the lifecycle manager for it.
This chapter goes beyond "launch it with a model" and covers the full surface:
the lifecycle, every launch flag, edge tuning, multimodal, and secure exposure.

The default engine is llama.cpp's `llama-server`; Ollama is pluggable via
`--engine ollama`. (Installing `llama-server` itself is covered in
[Installing llama.cpp](llama-cpp.md).)

## Lifecycle

```sh
ferric server up      # launch + publish local/global registrations
ferric server status  # resolve identity/listener ownership + report HTTP health
ferric server adopt --pid <PID> # verify and upgrade a live schema-v1 record
ferric server doctor  # check engine-binary + model presence (and reachability)
ferric server down    # stop only a verified instance, then clean registrations
```

`up` refuses to launch when a local/global registration already exists or the
target port is occupied. For llama.cpp it also requires a regular model file,
a regular projector when supplied, and nonzero context/port values. Ferric
retains the spawned child until its engine-specific HTTP endpoint returns 200;
it then binds the child to its process creation identity and verifies that child
owns the expected listener. Only after those checks does it publish the same
**runfile** bytes locally at `.ferric/server.json` and in the user config
directory. This is what lets `ferric query` and `ferric bench`
**auto-discover** the server—no `--api-base` needed.

`doctor` is the pre-flight check; it validates presence *without* launching:

```console
$ ferric server doctor --engine llama-server --model ./model.gguf
[ok] engine binary `llama-server`
[MISSING] model `./model.gguf`
[info] no server running — `ferric server up` to start one
```

`status` reports the registered server and whether it is actually answering:

```console
$ ferric server status
[verified] local registration ...: engine=LlamaServer pid=36792 base_url=http://127.0.0.1:8080/v1 listener=owned-loopback http=healthy
resolved one managed server: pid=36792 base_url=http://127.0.0.1:8080/v1 aliases=2 stale=0
```

`status` succeeds only when schema-v2 process identity and listener ownership
are exact and the engine-specific local HTTP endpoint returns 200. A bare TCP
listener is not healthy, and a healthy HTTP response is never teardown
authority. `down` independently revalidates the retained process handle and
listener facts, waits for that exact process instance to exit, and only then
cleans its unchanged registrations.

A wildcard/public bind is reported explicitly and makes `status` fail. Because
the process generation is still exact, `down` may stop that same retained
process and verify listener release; it never treats public exposure as healthy
managed state. Native destructive lifecycle control is supported on Windows
and on little-endian 64-bit x86_64/AArch64 Linux. Other platforms fail closed
until they have an equivalent retained-process adapter.

## Registration and teardown safety

Schema v2 adds teardown identity without breaking deserialization of historical
runfiles. In addition to the engine, PID, port, and base URL, a new registration
records:

- a process-creation token that distinguishes PID reuse;
- the resolved executable and complete argument vector;
- the absolute path of the originating local `.ferric/server.json` mirror; and
- the existing launch provenance, including model and context when known.

Ferric inventories the current workspace's local registration and the user-level
global registration independently. A global schema-v2 record also identifies
its real originating local mirror, so running `status` or `down` from another
workspace does not silently replace that origin with the current directory.
Equivalent records are aliases of one managed instance; stale records are kept
separate. A malformed, unreadable, symlinked, conflicting, or otherwise
unverifiable entry makes resolution fail closed—no process is signalled.

Registration publication is no-clobber and atomic per path, not one
cross-filesystem transaction. If the second mirror cannot be published, Ferric
stops the child it still owns before conditionally rolling back the first.
Cleanup likewise removes only the exact bytes captured during resolution. If a
file changes or is replaced concurrently, the replacement is preserved and the
command reports partial cleanup.

`server down` has three practical outcomes:

- No registration: it is an idempotent success.
- Stale records only: when every registered endpoint is absent, it conditionally
  removes unchanged records and explicitly reports that no process was stopped;
  a live, foreign, shared, or uninspectable listener keeps the recovery record.
- One verified schema-v2 instance: it terminates only through the retained exact
  process handle, proves exit, and then conditionally cleans all matching aliases
  and stale records.

Multiple live identities, disagreement between process and listener ownership,
uninspectable state, or any blocked registration refuses teardown. Fix the
reported state; do not work around it with a broad process-name kill.

### Recovering a live schema-v1 registration

Historical schema-v1 runfiles remain readable, but a numeric PID alone cannot
authorize teardown. If that PID is live, both `status` and `down` retain the
record and print a copy/paste recovery command. From the workspace containing
the local record, run:

```sh
ferric server adopt --pid <PID>
```

Adoption is non-destructive. It acquires an exact retained process handle,
checks the closed engine executable and every available recorded argv
coordinate, requires exclusive IPv4-loopback listener ownership, and
conditionally replaces only unchanged local/global aliases with schema v2.
Any disagreement leaves the live process unsignalled and preserves recovery
bytes. A later `server down` re-acquires and revalidates the adopted generation.

If the legacy PID is already absent, run:

```sh
ferric server down
ferric server up --engine llama-server --model /path/to/model.gguf
```

The first command treats the unchanged legacy record as stale and removes it
without signalling a process; the second publishes schema v2. If the PID was
reused or the record is malformed, Ferric continues to refuse automatic
cleanup. In that case, compare the local and global files with the record you
inspected and remove only those exact unchanged files manually.

## Launch flags

`ferric server up` accepts:

| Flag | Default | Purpose |
|---|---|---|
| `--engine <llama-server\|ollama>` | `llama-server` | which engine to launch |
| `--model <PATH\|NAME>` | — | GGUF path (llama-server) or model name (Ollama) |
| `--mmproj <PATH>` | — | multimodal projector GGUF (image/audio/video) |
| `--ctx <N>` | `4096` | context window in tokens |
| `--port <N>` | `8080` | port to bind **on 127.0.0.1** |
| `--threads <N>` | engine default | CPU threads (llama-server only) |
| `--gpu-layers <N>` | engine default | layers to offload to GPU (llama-server only) |
| `--batch-size <N>` | engine default | batch size (llama-server only) |
| `--tailscale` | off | reserved; currently refused before any process or registration side effect |

### How the flags map to `llama-server`

Ferric builds a closed, audited command line — it never execs an arbitrary
binary (the engine is a fixed enum). The mapping is:

| Ferric flag | `llama-server` argument |
|---|---|
| `--model` | `-m <path>` |
| `--mmproj` | `--mmproj <path>` |
| `--ctx` | `-c <n>` |
| `--threads` | `-t <n>` |
| `--gpu-layers` | `-ngl <n>` |
| `--batch-size` | `-b <n>` |
| `--port` | `--port <n>` |
| (fixed) | `--host 127.0.0.1` |

For Ollama, `--engine ollama` runs `ollama serve` with `OLLAMA_HOST` set to the
chosen host:port; the llama-server-only tuning flags are ignored.

## Loopback-only, by design

The host is **pinned to `127.0.0.1`** and is not configurable. The launcher never
binds a public interface. This is ADR-005: the harness and its backend talk only
over loopback, so an inference server Ferric launched is never reachable from off
the machine. This is also why, in the containerized topology, `ferric` and
`llama-server` are co-located in one container rather than split across a network
boundary (see [Container Topology & Roadmap](swarming-k8s.md)).

`server up --tailscale` is temporarily fail-closed. Ferric refuses it during
preflight, before spawning an engine or writing a registration, because it
cannot yet compare-and-remove only the durable Tailscale Serve state it created.

An existing registration with `tailscale: true` is also blocked from automatic
teardown, even when it appears stale. Inspect the exact Serve configuration with
`tailscale serve status` and independently verify the registered process
creation instance, executable, arguments, and listener owner. Stop that exact
process, then use the targeted removal syntax shown by `tailscale serve --help`
to remove only the endpoint for the recorded port. Never use a node-wide Serve
reset as lifecycle cleanup. Finally compare and remove only its unchanged
local/global registration files. A future release can restore `--tailscale`
after it can capture, own, and conditionally restore that external state.

## Edge tuning

`--threads`, `--gpu-layers`, and `--batch-size` exist for constrained targets —
Jetson, Raspberry Pi, and similar. On a CPU-bound edge box, thread count is the
primary latency lever; on a small GPU, `--gpu-layers` decides how much of the
model is offloaded. A representative edge launch:

```sh
ferric server up --engine llama-server --model ./model.gguf \
  --ctx 8192 --threads 4 --gpu-layers 20 --batch-size 256
```

Tune, then **measure** — the [testbench](testbench.md) tells you whether a given
configuration still drives the tools reliably, which is the number that actually
matters for an agent.

## Multimodal

Pass `--mmproj` alongside `--model` to load a vision/audio projector; the model
can then read images and audio when the query declares `--modality`. See
[Multimodal Input](multimodal.md) for the full pipeline.

## Looking ahead: Animus Ophanim

Today the engine is external — llama.cpp via the HTTP valve — and this chapter is
about configuring *that*. The Animus Project's stated direction is a
**native-Rust inference engine, Animus Ophanim**, that will eventually replace
llama.cpp as the default backend and fold the runner into the harness's own
process and language.

> The name is deliberate. The *ophanim* are the wheels of the divine chariot —
> "wheels within wheels," full of eyes, that carry the whole apparatus and move
> wherever the spirit directs. An inference engine is exactly that: the wheels
> that actually move the body, nested computation within computation, all
> attention, going wherever the model's intent points. The spine drives; Ophanim
> is what it drives *with*.

Until Ophanim ships, everything in this chapter is how you get the most out of
the llama.cpp valve. **(Animus Ophanim is stated intent — not yet built.)**

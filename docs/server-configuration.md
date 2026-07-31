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
ferric server up      # launch + register (writes .ferric/server.json)
ferric server status  # health-check the registered server, print its base URL
ferric server doctor  # check engine-binary + model presence (and reachability)
ferric server down    # stop it and remove the runfile
```

`up` writes a **runfile** at `.ferric/server.json` recording the engine, PID,
port, and base URL. This is what lets `ferric query` and `ferric bench`
**auto-discover** the server — you launch once and everything else finds it, no
`--api-base` needed.

`doctor` is the pre-flight check; it validates presence *without* launching:

```console
$ ferric server doctor --engine llama-server --model ./model.gguf
[ok] engine binary `llama-server`
[MISSING] model `./model.gguf`
[warn] registered server http://127.0.0.1:8080/v1 is not reachable
```

`status` reports the registered server and whether it is actually answering:

```console
$ ferric server status
engine=LlamaServer pid=36792 base_url=http://127.0.0.1:8080/v1 (NOT reachable)
```

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
| `--tailscale` | off | expose the port over Tailscale Serve |

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

If you genuinely need remote access, `--tailscale` exposes the port over your
tailnet via Tailscale Serve (requires the `tailscale` CLI, and the first run on a
machine prints an authorization link you must click). This is an explicit,
authenticated opt-in — not a public bind.

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

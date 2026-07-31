# Credits & Citations

Animus Ferric stands on a great deal of open-source work and a handful of
published ideas. This page records both. License terms belong to each project;
follow the links for the authoritative text.

## Inference engines & infrastructure

| Project | Role in Animus | License |
|---|---|---|
| [llama.cpp](https://github.com/ggml-org/llama.cpp) (`llama-server`) | the default inference backend behind the HTTP valve | [MIT](https://github.com/ggml-org/llama.cpp/blob/master/LICENSE) |
| [llguidance](https://github.com/guidance-ai/llguidance) | grammar-constrained decoding inside the server (the mechanism behind harness-owned decoding) | [MIT / Apache-2.0](https://github.com/guidance-ai/llguidance) |
| [Ollama](https://github.com/ollama/ollama) | pluggable alternative engine (`--engine ollama`) | [MIT](https://github.com/ollama/ollama/blob/main/LICENSE) |
| [gVisor](https://github.com/google/gvisor) (`runsc`) | microVM-class sandbox runtime for Ornstein's airlock | [Apache-2.0](https://github.com/google/gvisor/blob/master/LICENSE) |
| [Docker](https://www.docker.com/) | container topology for the platform (see [Swarming](swarming-k8s.md)) | [Apache-2.0 / proprietary components](https://www.docker.com/legal/) |
| [Tailscale](https://tailscale.com/) | optional secure remote exposure (`--tailscale`) | [BSD-3-Clause (core)](https://github.com/tailscale/tailscale/blob/main/LICENSE) |
| [mdBook](https://github.com/rust-lang/mdBook) | builds this book from `docs/` | [MPL-2.0](https://github.com/rust-lang/mdBook/blob/master/LICENSE) |

## Rust dependencies

The harness itself is Rust. Its direct external crates, each on
[crates.io](https://crates.io) with its own license (the ecosystem norm is
`MIT OR Apache-2.0`):

- **CLI & runtime:** [`clap`](https://crates.io/crates/clap),
  [`tokio`](https://crates.io/crates/tokio),
  [`futures-executor`](https://crates.io/crates/futures-executor),
  [`rustyline`](https://crates.io/crates/rustyline)
- **Serialization & text:** [`serde`](https://crates.io/crates/serde),
  [`serde_json`](https://crates.io/crates/serde_json),
  [`toml`](https://crates.io/crates/toml),
  [`regex`](https://crates.io/crates/regex),
  [`chrono`](https://crates.io/crates/chrono)
- **HTTP & server:** [`reqwest`](https://crates.io/crates/reqwest) (with
  `rustls-tls`, no native OpenSSL), [`axum`](https://crates.io/crates/axum),
  [`tokio-stream`](https://crates.io/crates/tokio-stream)
- **Errors, async, temp, observability:**
  [`thiserror`](https://crates.io/crates/thiserror),
  [`async-trait`](https://crates.io/crates/async-trait),
  [`tempfile`](https://crates.io/crates/tempfile),
  [`tracing`](https://crates.io/crates/tracing) +
  [`tracing-subscriber`](https://crates.io/crates/tracing-subscriber)
- **Prompt composition:** [`oovra`](https://github.com/crussella0129/oovra) — a
  sibling Animus Project library, rev-pinned.

## Papers & ideas

The design draws directly on published work:

- **ReAct** — Yao et al., *ReAct: Synergizing Reasoning and Acting in Language
  Models*, 2022 ([arXiv:2210.03629](https://arxiv.org/abs/2210.03629)). The
  reason-then-act loop that agentic harnesses, Animus included, are built around.
- **CaMeL** — Debenedetti et al. (Google DeepMind), *Defeating Prompt Injections
  by Design*, 2025 ([arXiv:2503.18813](https://arxiv.org/abs/2503.18813)). The
  capabilities/taint model behind Ferric's "CaMeL-lite" sink policy
  (`--sink-action`).
- **ICM** — Van Clief & McDermott, *Interpretable Context Methodology: Folder
  Structure as Agent Architecture*, 2026. The delegation model where the
  filesystem *is* the orchestration layer; adapted in [ICM — Agent
  Delegation](icm.md).
- **The lethal trifecta** — Simon Willison,
  [*The lethal trifecta for AI agents*](https://simonwillison.net/2025/Jun/16/the-lethal-trifecta/),
  2025. The private-data + untrusted-content + exfiltration framing that
  [Ornstein](ornstein.md)'s structural quarantine is designed against.

> If you believe an attribution here is incorrect or incomplete — a wrong
> license, a missing project, a citation that should point elsewhere — that is a
> bug in this page; please open an issue.

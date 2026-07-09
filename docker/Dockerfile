# ferric-core — the one container that co-locates the `ferric` binary and its
# inference backend (`llama-server`). See ADR-051 (sprint 41).
#
# Why co-located, not two containers: `ferric server` pins the backend to
# loopback (127.0.0.1) by ADR-005 — splitting `ferric` and `llama-server`
# across separate containers would break that loopback guarantee without extra
# network-namespace plumbing. Keeping them in one container preserves today's
# guarantee unchanged.
#
# Scope-limits (deliberate, ADR-051): x86_64/Linux only for this skeleton
# (multi-arch via `buildx --platform` is the deferred future path); no EXPOSE /
# published port (loopback-only preserved structurally). Ornstein and any
# future chat/MCP surfaces are SEPARATE sibling services, never nested here.

# ---- Stage 1: build the `ferric` binary --------------------------------------
# Pinned to the workspace toolchain (rust-toolchain.toml: 1.96). The `-bookworm`
# tag is LOAD-BEARING: it must match the runtime stage's Debian release so the
# built binary's GLIBC requirement (dynamic-linked) is satisfiable at runtime.
# A bare `rust:1.96-slim` tracks a newer Debian (trixie, GLIBC 2.39) and the
# resulting `ferric` fails on bookworm with "GLIBC_2.39 not found" — caught only
# by actually RUNNING the image (T-4105 live validation), not by `docker build`
# succeeding.
FROM rust:1.96-slim-bookworm AS build

# --features backend-openai is MANDATORY: ferric-cli's `default = []`, so a
# plain release build produces a `ferric` that CANNOT drive the co-located
# llama-server over the OpenAI HTTP valve (ADR-051). rustls-tls (ADR-045) means
# no OpenSSL system dep is needed for the build.
WORKDIR /src
COPY . .
RUN cargo build --release --features backend-openai -p ferric-cli

# ---- Stage 2: runtime — `ferric` + a prebuilt `llama-server` -----------------
# Slim Debian; only what's needed to run the two binaries.
FROM debian:bookworm-slim AS runtime

# curl to fetch the prebuilt llama-server release; ca-certificates for TLS (the
# OpenAI valve talks HTTPS to remote servers when not using the local one).
# libgomp1 is llama.cpp's OpenMP runtime dependency.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl libgomp1 \
    && rm -rf /var/lib/apt/lists/*

# Prebuilt llama-server (docs/llama-cpp.md §1 — asset-selection-by-hardware; the
# Linux x64 CPU asset here, matching this skeleton's x86_64 scope-limit). Pinned
# to the b9821 release validated live in sprints 23–26. No build-from-source: a
# single minimal binary + its ggml/llama shared libs, all flat in one dir (the
# whole point of the llama.cpp engine, ADR-032). LD_LIBRARY_PATH lets
# llama-server find its co-located .so files.
ARG LLAMA_RELEASE=b9821
RUN curl -fsSL -o /tmp/llama.tar.gz \
      "https://github.com/ggml-org/llama.cpp/releases/download/${LLAMA_RELEASE}/llama-${LLAMA_RELEASE}-bin-ubuntu-x64.tar.gz" \
    && tar -xzf /tmp/llama.tar.gz -C /opt \
    && rm /tmp/llama.tar.gz
ENV PATH="/opt/llama-${LLAMA_RELEASE}:${PATH}" \
    LD_LIBRARY_PATH="/opt/llama-${LLAMA_RELEASE}"

# The compiled harness binary.
COPY --from=build /src/target/release/ferric /usr/local/bin/ferric

# A non-root user owns the workspace mount (least privilege inside the
# container; the real containment boundary is still ferric-guard + ADR-005).
RUN useradd --create-home --uid 10001 ferric
USER ferric
WORKDIR /workspace

# No EXPOSE: `ferric server` binds llama-server to loopback INSIDE this
# container only (ADR-005) — nothing is published to the host or other
# containers. Deliberate (ADR-051).

# Default: show help. Real invocations override the command
# (e.g. `ferric server up ...` then `ferric query ...`), typically via
# docker-compose (see docker/docker-compose.yml).
ENTRYPOINT ["ferric"]
CMD ["--help"]

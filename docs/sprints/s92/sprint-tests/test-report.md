# Sprint 92 — Test Report

## Gate

| Check | Result |
|---|---|
| `cargo test --workspace` | **535 passed / 0 failed** |
| clippy / fmt | 0 warnings / clean |

## The debugging result that changed the sprint

Before building an allowlist proxy, I checked whether the mechanism it plugs into
constrains anything. It does not:

```
# bridge + http_proxy pointing at a dead port, then bypassed:
$ docker run --network bridge --env http_proxy=http://127.0.0.1:9 alpine \
    sh -c 'unset http_proxy https_proxy; wget -qO- http://example.com'
<!doctype html><html lang="en"><head><title>Example Domain</title>...
```

Full page fetched. Proxy env vars are a **convention**, not a boundary.

## The foundation that does hold

```
# --internal network, same bypass attempt:
wget: bad address 'example.com'
# and by raw IP, removing DNS from the question:
wget: can't connect to remote host (93.184.215.14): Network unreachable
```

Kernel routing. Nothing inside the container to opt out of.

## Unit

`the_airlock_uses_the_isolated_network_not_bridge` — the critical assertion is
the **negative** one: the argv must never contain `--network bridge`, because
that one word is what restored the bypass.

## Live — the full topology, stood up by the test itself

Isolated network + gateway container on both networks + default-deny allowlist:

| Case | Result |
|---|---|
| allowlisted host via gateway | **fetched** (`Example Domain`) |
| non-allowlisted host via gateway | **403**, refused by the gateway |
| `unset http_proxy`, go direct | **unreachable** — bypass closed |

13.7 s, no SKIP line. The positive assertion (real page content) is the
anti-skip guard — ADR-081 nearly shipped a report where 5 skips read as 5 passes,
so every live suite here now has at least one assertion that cannot pass without
the dependency actually working.

## What is NOT tested, because it does not exist yet

Ferric neither creates the isolated network nor runs the gateway — the test does
that itself. "The type supports it" is not "the binary does it", and the
lifecycle is the next increment.

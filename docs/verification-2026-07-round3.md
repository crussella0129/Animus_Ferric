# Live-Model Verification — sprint 86

**Date:** 2026-07-25
**Scope:** the gap ADR-075 named as the highest-value next investment — nothing
in `Animus_Ferric` had met a real model since ~sprint 26, so all 503 tests were
mock-driven.
**Engine:** `ferric server` → llama.cpp `llama-server`, model
`qwen2.5-coder-7b-instruct-q4_k_m.gguf` (4.4 GB), ctx 8192, `--gpu-layers 99`.
**Also verified:** the `--tailscale` path, against the real Tailscale CLI (1.98.2)
on a connected node.

---

## 0. What worked, live

The whole server lifecycle and the constrained loop work end-to-end against a
real model. This is the first live confirmation in ~60 sprints.

| Step | Result |
|---|---|
| `ferric server doctor` | `[ok]` engine binary, `[ok]` model, correctly warns the registered server is unreachable |
| `ferric server up --engine llama-server --model … --ctx 8192 --gpu-layers 99` | loads, binds loopback, writes `.ferric/server.json` |
| `ferric server status` | `engine=LlamaServer pid=51704 base_url=http://127.0.0.1:8080/v1 (reachable)` |
| `ferric server down` | `stopped server pid 51704`, runfile removed |
| Constrained agentic loop | **read_file → write_file → task_complete, 3 turns, 36 s**, correct result |
| Streaming | token-by-token output rendered live |
| Trace | complete JSONL written per session |

The task was *"read notes.txt and write summary.txt containing exactly the number
of lines it has."* The model read the file, wrote `3`, and terminated correctly.
`summary.txt` contained `3`.

---

## 1. F1 — PROVEN — the guard family has an A-B-A-B oscillation hole

Given a slightly harder task, qwen2.5-coder-7b fell into a two-cycle and burned
the **entire 20-turn budget**:

```
total tool calls: 20
distinct (name,args) pairs: 2
    find_files   {"max_results": 1, "path": ".", "pattern": "big.txt"}
    search_files {"path": "big.txt", "query": "line"}
GUARD EVENTS FIRED: NONE
```

Ten identical `search_files` calls and ten identical `find_files` calls,
alternating, and **not one of the three guards fired**.

**Why all three miss it.** Every guard keys on *consecutive-turn* state, so
alternation resets each one every turn:

- **RepetitionGuard** (ADR-027) hashes name+args of the turn's calls. Turn N is
  `search_files`, turn N+1 is `find_files` — never two identical turns in a row,
  so the streak never reaches its threshold of 2.
- **ProgressGuard** (ADR-037) canonicalizes the sorted-unique tool *names* per
  turn. `{search_files}` then `{find_files}` — different again, so the streak
  resets before reaching 5.
- **FailureGuard** (ADR-038) counts all-errored turns. **These calls succeed**
  (191 and 7 chars returned), so it never engages at all.

The guard family's stated purpose is bounding wasted compute (ADR-037/038,
"honestly scoped: they bound wasted compute, they do not lift a capability
ceiling"). A 2-cycle of *successful* calls is precisely wasted compute, and it
passes through untouched.

**Proposed fix:** a windowed guard — a multiset of `(name, args)` over the last
N turns, tripping when distinct-call-count stays low while turn-count climbs
(here: 2 distinct calls across 20 turns). Keep the existing three for the fast
paths; they catch their cases in 2–5 turns, which a window cannot.

**Only a live model finds this.** A scripted mock emits exactly what the test
author wrote; it never spontaneously oscillates. This is the concrete argument
for the live round.

## 2. F2 — PROVEN — `--tailscale` never discovers the Tailnet FQDN

`server.rs` read `json.get("DNSName")` from the root of `tailscale status
--json`. Against the real CLI on a connected node:

```
top-level keys: [AuthURL, BackendState, CertDomains, ClientVersion,
                 CurrentTailnet, HaveNodeKey, Health, MagicDNSSuffix,
                 Peer, Self, TailscaleIPs, User, Version]
top-level DNSName present?: False
Self.DNSName: tec-xx.tail944782.ts.net.
```

**`DNSName` lives under `Self`.** Reading it from the root always returned
`None`, the `&& let Some(...)` chain short-circuited, and `base_url` silently
stayed `http://127.0.0.1:8080/v1` — so `ferric server up --tailscale` printed
*"Tailscale proxy active."* and then wrote the **loopback** URL into the runfile.
Anything discovering the server through that runfile got 127.0.0.1, which defeats
the flag's entire purpose. ADR-060 and `docs/commands.md` both advertise this as
working.

**Fixed in this sprint** (extracted to a testable `tailnet_fqdn()` reading
`Self.DNSName`, trailing dot stripped, with a warning instead of silence when
discovery fails). 4 tests, including one pinning that the real payload has no
top-level `DNSName`.

**The other half of the integration is correct** and now has a test saying so:
`tailscale serve --bg <port>` is exactly the documented background form, verified
against `tailscale serve --help` on 1.98.2.

> `tailscale serve` was **not** executed. Running it would publish this machine's
> inference port to the tailnet persistently — an outward-facing change that is
> the user's call, not a verification step. Everything above was established from
> read-only `status`/`--help` output plus the code.

---

## 3. What the live round did *not* validate

Stated plainly, because a live round that quietly skips things is worse than no
live round.

- **A1's truncation cap was never exercised.** qwen chose to paginate `read_file`
  with `start_line`/`end_line` (60 chars returned), and `search_files` returned
  191. The 4,000-char cap is reachable mainly via a file with very long *lines*,
  or a large `shell_exec` — whose own limit is 10,000 chars, i.e. **above** the
  model-facing cap. Needs a targeted case.
- **A2's taint set** — `--research` was not run against the live model, so the
  E2 false-positive finding from ADR-075 stands unmeasured against real digests,
  and its posture decision is still open.
- **A5's sandbox** — Docker is still absent on this machine; only `docker_args()`
  is tested.
- **The fleet capability map** — no re-calibration was run; `measured_level`
  figures still date from sprints 25–26.

---

## 4. Suggested order

1. **F1** — close the oscillation hole. It is the one finding that costs users
   real time and money on every run that hits it.
2. **F2** — landed this sprint.
3. **A targeted A1 live case** (long-line file, or a `shell_exec` over the cap).
4. **E1/E4** from ADR-075 — still open, still small.
5. **E2** — the taint posture decision, ideally with live `--research` evidence.

---

## 5. What this round says about the mock suite

503 mock-driven tests, all green, did not surface either finding — and could not
have. F1 requires a model that *chooses* badly; F2 requires the actual
`tailscale` binary. Both were invisible by construction.

That is not an argument against the mock suite, which is fast and catches
regressions. It is an argument that **its green is evidence about a narrower
thing than it appears to be**, and that a live round belongs in the rotation
rather than once every sixty sprints.

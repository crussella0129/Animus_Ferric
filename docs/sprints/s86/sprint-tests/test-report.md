# Sprint 86 — Test Report (live-model round)

## Gate

| Check | Result |
|---|---|
| `cargo test --workspace` | **507 passed / 0 failed** (503 at sprint start, +4) |
| `cargo clippy --workspace --all-targets` | 0 warnings |
| `cargo fmt --all --check` | clean |

## Live stack — `ferric server` + llama.cpp + qwen2.5-coder-7b

| Step | Result |
|---|---|
| `server doctor` | `[ok]` engine binary, `[ok]` model, correctly warns unreachable |
| `server up --engine llama-server --ctx 8192 --gpu-layers 99` | loads 4.4 GB, binds loopback, writes runfile |
| `server status` | `pid=51704 base_url=http://127.0.0.1:8080/v1 (reachable)` |
| `server down` | `stopped server pid 51704`, runfile removed |
| Constrained loop | **read_file → write_file → task_complete, 3 turns, 36 s** |
| Result correctness | `summary.txt` == `3` (correct) |
| Streaming / trace | both working |

## F1 — guard oscillation hole (PROVEN live)

```
total tool calls: 20
distinct (name,args) pairs: 2
    find_files   {"max_results": 1, "path": ".", "pattern": "big.txt"}
    search_files {"path": "big.txt", "query": "line"}
GUARD EVENTS FIRED: NONE
```

Full 20-turn budget burned on a 2-cycle of **successful** calls. All three guards
key on consecutive-turn state, so alternation resets them; `FailureGuard` never
engages because nothing errors.

## F2 — tailscale FQDN (PROVEN, fixed)

Against the real CLI (1.98.2, connected node):

```
top-level DNSName present?: False
Self.DNSName: tec-xx.tail944782.ts.net.
```

4 new tests: FQDN read from `Self.DNSName`; the root has no `DNSName` (the
regression pinned); unparseable/absent/blank yield `None` rather than
`https:///v1`; and `serve --bg <port>` matches the documented form.

`tailscale serve` itself was **not executed** — it would publish this machine's
port to the tailnet persistently, which is the user's call. Read-only
`status`/`--help` was sufficient to find and fix the bug.

## Not validated this round

- **A1's truncation cap** — never exercised; the model paginated `read_file`
  (60 chars) and `search_files` returned 191. Reachable mainly via long *lines*
  or a large `shell_exec` (whose 10,000-char cap sits above the 4,000 model cap).
- **A2's taint set** — `--research` unrun against the live model; ADR-075's E2
  posture decision still open and still unmeasured on real digests.
- **A5's sandbox** — Docker still absent.
- **Fleet calibration** — `measured_level` figures still from sprints 25–26.

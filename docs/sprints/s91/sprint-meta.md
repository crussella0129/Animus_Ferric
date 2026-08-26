# Sprint 91 Meta

- **Sprint number:** 91
- **Start timestamp:** 2026-07-25T13:56:32Z
- **End timestamp:** 2026-07-25T14:55:00Z
- **Model:** claude-opus-5
- **Exit status:** success
- **Token count:** not observable
- **Summary:** Validate A5's sandbox airlock against a real Docker daemon —
  the work blocked since sprint 33.

## Outcome

**534 tests, 0 failures**; clippy 0; fmt clean.

The airlock is confirmed live, with direct evidence:

- `--network none` genuinely isolates — `wget: bad address 'example.com'`
- explicit `Unrestricted` genuinely fetches — `Example Domain`
- capabilities really drop — `chown` fails in-container
- **the gVisor default FAILS CLOSED** — with `runsc` absent this host errors
  (`unknown or invalid runtime name: runsc`) rather than falling back to `runc`

The last one is the property that makes the default worth having. A security
control that degrades silently is worse than none.

## A defect I hit myself

`check_available()` had no timeout. A *half-started* Docker Desktop makes
`docker info` **hang** rather than fail — ~60 s per call. Now bounded at 5 s with
the probe reaped. This is not test hygiene: `Retriever::available()` sits on the
research path, so an unbounded probe would stall a real run over an optional
dependency.

## A near-miss worth recording

The first run of the suite reported **"5 passed" in 269 s**, and I almost
reported it as validation. It was 5 *skips* — availability-gated, with SKIP lines
only visible under `--nocapture`. The real run takes **12 s**. The file's own
header warns against exactly this confusion, and it still nearly landed; the
timing was the only honest tell.

## Docker was not D2's blocker

`WebRetriever` remains unreachable from the binary — but the gap is the
**allowlist proxy**, not the containerizer. `NetworkPolicy::Proxy` exists and
nothing constructs it, so wiring the web plane today offers only `Denied` (cannot
fetch) or `Unrestricted` (the egress sprint 84 made opt-out). **The proxy is now
the single remaining blocker for Ornstein's web plane.**

## Next

The allowlist proxy, then D2 behind it. Then fleet re-calibration (nothing since
sprints 25–26) and C7/C8/B1.

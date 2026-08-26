# Sprint 94 Meta

- **Sprint number:** 94
- **Start timestamp:** 2026-07-25T15:48:30Z
- **End timestamp:** 2026-07-25T16:40:00Z
- **Model:** claude-opus-5
- **Exit status:** success
- **Token count:** not observable
- **Summary:** D2 — wire `WebRetriever` into the binary, open since the sprint-82
  audit.

## Outcome

**545 tests, 0 failures**; clippy 0; fmt clean; no leaked docker resources.

`--research-url <URL>` (repeatable) drives the web plane, separate from
`--research` because `research_all` hands one query to every plane and the two
want different things — Local-FS keywords, Web an exact URL. One flag meaning two
things would have been the quiet kind of wrong.

**The allowlist is derived from the URLs and nothing else**, so there is no
second source of truth to drift and the sandbox may reach precisely the hosts
named. That concentrates the security question in `url_host`, which strips
userinfo (`http://example.com@evil.test/` → `evil.test`), drops the port, and
validates before the airlock opens.

One airlock per run (~15 s, RAII). A failed fetch is fatal, not skipped —
ADR-078's lesson applied before it could recur. gVisor stays the default with
`--allow-standard-runtime` as a named opt-out, documented with the thing that
matters: **network isolation does not depend on it.**

## Verified live (qwen2.5-coder-3B)

- airlock opens for the named host; URL fetched, quarantined, summarised
- the same run's `write_file` is **denied** — contaminated run, no approver
- an unallowlistable URL is refused **before** the airlock opens

The middle one is what shows the chain composes: the provenance gate does its job
without knowing anything about the web plane.

## Where Ornstein stands

All three planes reachable from the binary: Local-FS, Tailnet-FS (still
unexercised live — no sshd on any tailnet host), Web.

## Next

Fleet re-calibration (nothing since sprints 25–26), a prebuilt gateway image to
cut the ~15 s airlock startup, and C7/C8/B1.

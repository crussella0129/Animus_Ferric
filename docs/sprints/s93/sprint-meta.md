# Sprint 93 Meta

- **Sprint number:** 93
- **Start timestamp:** 2026-07-25T14:50:40Z
- **End timestamp:** 2026-07-25T15:55:00Z
- **Model:** claude-opus-5
- **Exit status:** success
- **Token count:** not observable
- **Summary:** Give Ferric the airlock lifecycle — create the networks, run the
  gateway, tear it all down.

## Outcome

**542 tests, 0 failures** (parallel); clippy 0; fmt clean; zero leaked containers
or networks.

`airlock::Airlock::start(allowlist)` creates the `--internal` network and an
egress network, runs a default-deny gateway, polls until it accepts connections,
attaches it to the internal side, and yields a `NetworkPolicy::Airlock`.

## The decisions that carry weight

- **Attach order** — the gateway joins the internal network only *after* it is
  ready, so a sandbox can never reach a proxy that is listening but has not
  loaded its filter.
- **Poll, never sleep** — `apk add` timing varies; a sleep that is usually long
  enough is a flake generator. The poll also detects an *exited* gateway and
  reports its last log line.
- **RAII teardown** — a panic between `start` and `stop` would otherwise leak the
  one container on the machine that has egress.
- **Fail closed** — no path yields a sandbox with an open network.
- **Allowlist validation is a boundary** — entries are written by a shell inside
  the gateway, so `;` or `$(…)` would be injection into the one container with
  network access. Restricted to the DNS charset, checked before any docker
  resource exists.

## The failure this sprint produced

`dropping_the_airlock_removes_its_resources` **passed single-threaded and failed
in the parallel suite.** It asserted on the `ferric-gateway-` prefix, so a
concurrent test's gateway satisfied the post-drop check.

The teardown was correct; the assertion was not. Per-instance naming exists
*precisely* to make that distinction — and the first test written against it
reached for `contains` instead. Only parallelism exposed it, which is worth
remembering: single-threaded is the mode that hides cross-test interference, and
it is the mode one reaches for when a test looks flaky.

## Measured cost

`Airlock::start` ≈ 10–15 s, dominated by `apk add tinyproxy`. Fine for a test; a
real consideration for how often the web plane stands one up.

## D2

Unblocked, not done. The mechanism exists and is verified; what remains is wiring
— CLI surface, allowlist as configuration, and per-run vs per-query.

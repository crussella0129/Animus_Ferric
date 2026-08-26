# Sprint 93 — Test Report

## Gate

| Check | Result |
|---|---|
| `cargo test --workspace` | **542 passed / 0 failed** (parallel, default threads) |
| clippy / fmt | 0 warnings / clean |
| leaked containers / networks | **0 / 0** |

## Unit — allowlist validation (5 tests)

The security-relevant ones:

- `shell_metacharacters_are_rejected` — `;`, `$(…)`, backticks, newlines, quotes,
  spaces, pipes, wildcards. These reach a shell **inside the gateway**, the one
  container with egress, so this is injection prevention, not tidiness.
- `a_bad_allowlist_fails_before_touching_docker` — a rejected entry must not
  leave a half-built airlock behind.

## Live — the lifecycle (3 tests)

| Test | Asserts |
|---|---|
| `ferric_stands_up_an_enforcing_airlock` | allowlisted host **fetched**; non-allowlisted **refused**; `unset http_proxy` **unreachable** |
| `dropping_the_airlock_removes_its_resources` | `Drop` really removes **this** airlock's gateway |
| `an_injecting_allowlist_creates_nothing` | no networks created for a rejected allowlist |

The first case's positive assertion doubles as the anti-skip guard: it cannot
pass unless Docker, both networks, and the gateway all genuinely worked.

## The failure this sprint produced, and what it was

`dropping_the_airlock_removes_its_resources` **passed single-threaded and failed
in the full parallel suite.** It asserted on the `ferric-gateway-` prefix, so a
concurrently running test's gateway satisfied the "still exists" check after the
drop.

**The teardown was correct; the assertion was not.** Airlock names are unique per
instance precisely so this distinction is possible — and the first test written
against that scheme ignored it. Now asserts on `lock.gateway_name()`.

Worth recording because the single-threaded run was the misleading one: it is the
mode that hides cross-test interference, and it is the mode one reaches for when
a test looks flaky.

## Cost, measured

`Airlock::start` ≈ 10–15 s, dominated by `apk add tinyproxy` in a fresh
container. Fine for a test; a consideration for how often the web plane stands
one up.

---

## Amendment — CI failure on windows-latest (ADR-084)

The sprint's first CI run failed on `windows-latest` while passing on ubuntu.

**Cause:** that runner ships a Docker daemon in **Windows-container mode**.
`check_available()` asked "is a daemon reachable?", got yes, and every
`alpine:latest` run then died with
`no matching manifest for windows(10.0.26100)/amd64`.

**The part that matters:** *five of the six* sandbox tests **passed** on that
runner. They assert failure — denied network, dropped capabilities, absent
runsc, refused host — and a failed image pull is a failure. The container never
ran. Only `a_command_runs_inside_the_sandbox`, the one with a **positive**
assertion, caught it.

### Two fixes

1. `check_available()` requires `OSType == linux`. Every image here is Linux, so
   a Windows-mode daemon is not availability — and reporting it as such would
   make `Retriever::available()` promise a plane that cannot work. Product fix,
   not a CI workaround.
2. `assert_failed_because(result, reasons, what)` replaces bare `is_err()`. It
   rejects `no matching manifest` / `Unable to find image` outright (the
   container never ran, so the test proves nothing) and requires the error to
   match the reason under test — `bad address` for a denied network,
   `Operation not permitted` for capabilities, `runsc` for the runtime, `403`
   for a gateway refusal.

### The generalisable lesson

`assert!(result.is_err())` is a test of nothing in particular. It passes for the
intended reason and for every unintended one — and unintended failures are
exactly what a foreign environment supplies. This is the counterpart to ADR-081's
anti-skip guard: keep one assertion that cannot pass without the dependency
working, **and** make every negative assertion name its reason.

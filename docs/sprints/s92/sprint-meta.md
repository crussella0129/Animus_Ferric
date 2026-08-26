# Sprint 92 Meta

- **Sprint number:** 92
- **Start timestamp:** 2026-07-25T14:28:40Z
- **End timestamp:** 2026-07-25T15:30:00Z
- **Model:** claude-opus-5
- **Exit status:** success
- **Token count:** not observable
- **Summary:** Build the allowlist proxy — and find first that the mechanism it
  would plug into was advisory, not an airlock.

## Outcome

**535 tests, 0 failures**; clippy 0; fmt clean.

**The debugging result changed the sprint.** `NetworkPolicy::Proxy` attached the
sandbox to `--network bridge` and set `http_proxy`. Those variables are a
convention cooperative clients honour, not a boundary — a container runs
`unset http_proxy https_proxy` and fetches the internet in full, measured with
the proxy pointed at a dead port.

Building a careful allowlist proxy behind that would have been the most
convincing version of this series' recurring mistake: a lot of correct-looking
work enforcing nothing.

**Isolation now comes from the network.** `Proxy(url)` → `Airlock { network,
proxy_url }`, on a docker network created `--internal` — no route out, verified
against both DNS and raw IP so the result could not be a name-resolution
artefact.

## Verified end-to-end

Real topology, gateway on both networks, default-deny allowlist:

- allowlisted host → **fetched**
- non-allowlisted host → **403**, refused by the gateway
- `unset http_proxy`, go direct → **unreachable** — the bypass is closed

## Not done, said plainly

Ferric neither creates the isolated network nor runs the gateway; the live test
does that itself. **"The type supports it" is not "the binary does it."** That
lifecycle — create, start, health-check, attach, tear down, allowlist as
configuration — is the remaining piece before D2, and
`sandbox_live.rs::start_airlock` is the working reference.

## Method note

Every live suite here now carries at least one assertion that cannot pass unless
the dependency genuinely works (here: a real page body). ADR-081 nearly shipped a
report where 5 skips read as 5 passes; that guard is the fix for it.

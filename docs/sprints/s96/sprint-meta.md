# Sprint 96 Meta

- **Sprint number:** 96
- **Start timestamp:** 2026-07-25T16:56:41Z
- **End timestamp:** 2026-07-25T17:40:00Z
- **Model:** claude-opus-5
- **Exit status:** success
- **Token count:** not observable
- **Summary:** Prebuilt gateway image (ADR-083's follow-up) + C8. The
  measurement disproved the premise, and the verification found a worse bug.

## Outcome

**550 tests, 0 failures**; clippy 0; fmt clean; no leaked airlock resources.

## The premise did not survive measurement

ADR-083 recorded `Airlock::start` at "≈ 10–15 s, dominated by `apk add`". Timed
before and after on the same machine:

| | `Airlock::start` |
| --- | --- |
| old (`apk add` per start) | **2.72 s** |
| new (prebuilt, warm) | **2.08 s** |
| new (cold — builds the image) | 4.52 s, once |

0.6 s. The 10–15 s came from a suite run that included the alpine pull, written
down as the cost of `start` and then read as fact for three sprints.

The change is still right, for a different reason: every start used to fetch a
package from the Alpine mirror, so **the security boundary had a runtime
dependency on an external mirror.** That is now a one-off build.
`the_gateway_image_needs_no_package_fetch` pins it with `--network none`.

## The bug the verification found

Planting a `tinyproxy`-less image to confirm the new test *could* fail — the
ADR-079 discipline — the suite went **green**. All three live tests skipped on a
failed `Airlock::start`, having already passed `docker_ready()`. Docker worked,
the airlock was broken, and the tests reported success.

**The availability gate belongs at the top of a test, not around its subject.**
`start_or_fail` now panics. Planted image: 3 failed, 1 passed. Real image: 4
passed.

Sharper than ADR-079's rule: it is not enough to carry one assertion that needs
the dependency — the *skip conditions* must not be able to fire after the gate.

Also shown incidentally: three failed starts left zero gateways and zero
networks, so cleanup on a **partial** start holds. Only the success path had
been demonstrated before.

## C8

Two loose runners moved to `tools/`; `run-tool-sweep.ps1` deleted (read a
prompt file that only exists in `workspace/` — unrunnable as placed). The
audit's third location was a miscount: `workspace/run-e2e-sweep.sh` sits in the
directory mounted at `/workspace`, beside the fixtures it reads by absolute
path. `tools/README.md` records why both shell dialects stay.

## Not mine

`docker-ferric-core-1` (compose, dated 2026-07-14, crash-looping) predates this
session. Left alone — surfaced to the user rather than removed.

## Next

C7 (`ferric-cli`, 19 flat modules) and B1 (`Protocol`'s dead variants — a schema
change with a migration). Tailnet-FS remains unexercised live; no sshd on any
tailnet host.

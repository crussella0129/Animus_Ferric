# Sprint 97 Meta

- **Sprint number:** 97
- **Start timestamp:** 2026-07-25T19:32:38Z
- **End timestamp:** 2026-07-25T20:15:00Z
- **Model:** claude-opus-5
- **Exit status:** success
- **Token count:** not observable
- **Summary:** The compose stack had never worked. Fixing it turned up five
  defects, and the first fix created the bug that broke the fifth.

## Outcome

**551 tests, 0 failures**; clippy 0; fmt clean. ADR-088.

## What was wrong

`docker-ferric-core-1` had been crash-looping since **2026-07-14**, spotted in
passing during sprint 96's leak check.

1. **The crash loop.** `ENTRYPOINT ["ferric"]` + `command: tail -f /dev/null`
   ran `ferric tail -f /dev/null` — compose's `command` replaces `CMD`, not
   `ENTRYPOINT`. Eleven days of `restart: unless-stopped`.
2. **The docker socket mount.** Doubly non-functional (no docker CLI in the
   image; socket `root:root 0660` vs uid 10001) while staging a
   host-root-equivalent escape beside the airlock. Removed.
3. **The squid `allowlist-proxy`.** On a **bridge** network — the topology
   ADR-082 proved bypassable — with a hardcoded allowlist, referenced by no
   code. Deleted with `squid.conf`.
4. **A tracked `.env`** pinning `MODELS_PATH` to one machine's NAS drive,
   overriding the default on every checkout. Untracked, gitignored,
   `.env.example` added.
5. **`ferric server down` could not kill anything in a slim image.** `kill(1)`
   is absent without `procps`, so the spawn failed, `down` said "already gone?",
   and llama-server kept serving — while the runfile was deleted anyway,
   orphaning it with no record. Now routes through `sh -c` (`kill` is a shell
   builtin) and **keeps the runfile when the process is still alive**.

## The coupling

Fix 1 made `sleep` PID 1. `sleep` does not reap, so the killed server became a
permanent **zombie** (`state=Z ppid=1`) — and a zombie answers `kill -0`, which
would have made fix 5's new "keep the runfile while alive" logic refuse to clean
up a dead process forever. `init: true` plus a `/proc/<pid>/stat` state check
closes both halves.

These failure modes are coupled through the container's process model. No unit
test observes that.

## The measurement error, again

An early check ran `ps … || echo 'none running'` and printed "none running"
while the server was serving: `ps` is not installed, so the `||` fired. Same
shape as ADR-086/087 — **a negative result whose reason went unverified.**
`/proc` settled it.

## Verified live

Stack up and stable; PID 1 is `docker-init`; both mounts good; socket confirmed
absent. A full constrained run **inside the container** against the shared-store
3B: `write_file` → `task_complete` in 2 turns, landing on the host through the
bind mount — the first work this stack has ever done. Then `server down`:
"stopped server pid 20", no `/proc` entry, no zombie, port closed, runfile
removed.

## Models

Both GGUFs moved to the shared suite store `C:\Users\charl\Animus\Models` (byte
sizes verified), with a `.gitignore` written **before** they landed — that repo
had none, so the first `git add -A` would have staged 6.6 GB.

## Tailnet

`TailnetFsRetriever` is still live-unexercised, now with a precise reason: no
tailnet peer has Tailscale SSH enabled, and ZimaBoard2 — the only online Linux
— refuses port 22, so it has no system sshd either. Unblocking is
`tailscale up --ssh` **on that machine**.

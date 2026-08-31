# The Inference Server

Animus does not contain a model runner. It drives an **OpenAI-compatible HTTP
server** — the "valve" — and `ferric server` is the lifecycle manager for it.
This chapter goes beyond "launch it with a model" and covers the full surface:
the lifecycle, every launch flag, edge tuning, multimodal, and secure exposure.

The default engine is llama.cpp's `llama-server`; Ollama is pluggable via
`--engine ollama`. (Installing `llama-server` itself is covered in
[Installing llama.cpp](llama-cpp.md).)

## Lifecycle

```sh
ferric server up      # launch + publish local/global registrations
ferric server status  # resolve identity/listener ownership + report HTTP health
ferric server adopt --pid 6501  # example: run the exact command status/down prints
ferric server doctor  # check engine-binary + model presence (and reachability)
ferric server down    # stop only a verified instance, then clean registrations
```

`up` refuses to launch when a local/global registration already exists or the
target port is occupied. For llama.cpp it also requires a regular model file,
a regular projector when supplied, and nonzero context/port values. Ferric
retains the spawned child until its engine-specific HTTP endpoint returns 200;
it first binds that child to a retained process object, then polls readiness,
revalidates the same process generation, and verifies that child owns the
expected listener. Only after those checks does it serialize the **runfile**
once. Each configured scope gets an exclusive same-directory stage; Ferric
writes, flushes, and file-syncs the complete bytes, atomically commits the stage
without replacing an existing final name, and syncs parent-directory metadata
where the platform supports it. Success is reported only after every configured
final contains parseable, byte-identical bytes. This is what lets `ferric
query` and `ferric bench` **auto-discover** the server—no `--api-base` needed.

Publication has a deliberately bounded durability claim. File contents are
synced before commit. Unix also syncs the containing directory after the final
name appears. Rust's portable file API cannot flush Windows directory metadata,
so Windows currently claims the file-level boundary only, not unqualified
power-loss durability for the directory entry.

`doctor` is the pre-flight check; it validates presence *without* launching.
Pure argument blockers and blocked registration state precede all external
probes. Otherwise doctor resolves the complete registration inventory first.
Degraded, stale-only, conflicting, or unverifiable state returns before
engine-version and model-file probes. With `--tailscale`, a valid preflight
runs the usual engine/model checks plus bounded read-only Tailscale LocalAPI
identity and Serve-configuration checks; it does not mutate Serve state:

```console
$ ferric server doctor --engine llama-server --model ./model.gguf
[ok] engine binary `llama-server`
[MISSING] model `./model.gguf`
[info] no server running — `ferric server up` to start one
```

`status` reports the registered server and whether it is actually answering:

```console
$ ferric server status
[captured] local registration C:\Users\<you>\server-project\.ferric\server.json: schema=2 engine=LlamaServer pid=36792 base-url=http://127.0.0.1:8080/v1 recorded-identity=... observed-identity=... listener=owned-loopback health=healthy
[captured] global registration %APPDATA%\ferric\server.json: schema=2 engine=LlamaServer pid=36792 base-url=http://127.0.0.1:8080/v1 recorded-identity=... observed-identity=... listener=owned-loopback health=healthy
[captured] origin registration C:\Users\<you>\server-project\.ferric\server.json promised-by=global registration %APPDATA%\ferric\server.json: schema=2 engine=LlamaServer pid=36792 base-url=http://127.0.0.1:8080/v1 recorded-identity=... observed-identity=... listener=owned-loopback health=healthy
[state] ready pid=36792 aliases=2 stale=0 listener=owned-loopback health=healthy
[next] managed server is ready at http://127.0.0.1:8080/v1; continue with the intended Ferric command and omit `--api-base` to use it
```

`status` prints one row for every configured local and global scope and every
origin promised by a captured global record, including absent or blocked rows.
Persisted identity and observed identity remain separate, and every report ends
with exactly one next safe action. It succeeds only when schema-v2 process
identity and listener ownership are exact and the engine-specific local HTTP
endpoint returns 200. HTTP is attempted only after ownership resolves to one
exclusive loopback target; Ferric keeps the retained process object across that
request and reinspects it afterward. A bare TCP listener is not healthy, and a
healthy HTTP response is never teardown authority. This engine-specific request
is the discovery health probe; a backend consumer subsequently makes its own
operation-specific HTTP request after endpoint selection. That later consumer
probe does not change lifecycle state or add teardown authority. `down`
independently revalidates the retained process handle and listener facts. It may
signal only the exact schema-v2 process generation when the expected listener
is either exclusively owned by that process or proven absent, regardless of
whether discovery health is healthy, unhealthy, or not probed. It then waits for
that exact process instance to exit and only afterward cleans its unchanged
registrations.

A wildcard/public bind is reported explicitly and makes `status` fail. Because
listener ownership is non-exclusive, `down` also refuses to signal it and keeps
the registration for recovery even when the process generation is exact. Native
destructive lifecycle control is supported on Windows and on little-endian
64-bit x86_64/AArch64 Linux. Other platforms fail closed until they have an
equivalent retained-process adapter.

## Registration and teardown safety

Schema v2 adds teardown identity without breaking deserialization of historical
runfiles. In addition to the engine, PID, port, and base URL, a new registration
records:

- a process-creation token that distinguishes PID reuse;
- the resolved executable and complete argument vector;
- the absolute path of the originating local `.ferric/server.json` mirror; and
- the existing launch provenance, including model and context when known.

A Tailscale launch additionally records a validated ownership object: its
128-bit path token, exact mount and loopback proxy target, canonical self FQDN,
remote `/v1` base, HTTPS port, and capture provenance. The mirrored record is
published before the Serve mutation, making it a write-ahead recovery journal.
The ordinary `base_url` stays loopback-local for normal Ferric discovery.

Ferric inventories the current workspace's local registration and the user-level
global registration independently. A global schema-v2 record also identifies
its real originating local mirror, so running `status` or `down` from another
workspace does not silently replace that origin with the current directory.
Equivalent records are aliases of one managed instance; stale records are kept
separate. A malformed, unreadable, symlinked, conflicting, or otherwise
unverifiable entry makes resolution fail closed—no process is signalled.
Aliases require both the same verified process key and structural equality of
all canonical runfile metadata. Matching PIDs or start tokens alone are not
enough. A stale peer may coexist with one selected live process only when its
remaining listener is absent or is fully accounted for by that selected
process.

Read-only backend selection consumes this same typed result. An explicit
`--api-base` remains an explicit endpoint and does not inspect registrations.
Commands with an explicit workspace resolve the local registration from that
selected workspace rather than from the launcher process's current directory.
Without an explicit endpoint, only an empty inventory selects the built-in
default and only a Ready inventory selects the managed endpoint. Degraded,
stale-only, conflicting, malformed, missing-origin, or ownership-uninspectable
state never falls through to the built-in endpoint. Strict autonomy always
requires Ready managed discovery even when `--api-base` is supplied, and final
validation repeats static and process discovery and requires the same process,
metadata, aliases, origins, and exact registration revisions before its final
discovery health probe. Only after that check may the consumer issue its own
HTTP request.

Registration publication is no-clobber and atomic per path, not one
cross-filesystem transaction and not simultaneous visibility across scopes. A
partial commit, committed-but-durability failure, or child exit during
publication enters compensation through the retained child object. Ferric must
prove that exact generation exited, reap it, and prove the listener absent
before removing a final or attempt stage. If signal or wait cannot prove exit,
all published finals remain as recovery clues. Once cleanup is authorized,
Ferric compare-removes only unchanged attempt-owned finals, preserves any
concurrent replacement, and explicitly cleans attempt stages. Every final and
stage receives a result; cleanup errors identify all preserved paths. Partial
compensation remains a failed launch rather than a partially registered live
server. A signal error never authorizes cleanup by itself; Ferric still waits
on the exact retained object, and only a successful retained wait can
independently prove that generation exited.

Later teardown cleanup likewise removes only the exact bytes captured during
resolution. If a file changes or is replaced concurrently, the replacement is
preserved and the command reports partial cleanup.

`server down` has three practical outcomes:

- No registration: it is an idempotent success.
- Stale records only: when every registered endpoint is absent, it conditionally
  removes unchanged records and reports `stale-cleaned` without signalling a
  process; a live, foreign, shared, wildcard, or uninspectable listener keeps
  every recovery record.
- One verified schema-v2 instance: it terminates only through the retained exact
  process handle. It reports `stopped` only after proving that handle exited and
  all registered listeners were released; only then does it conditionally clean
  the target, matching aliases, and reconciled stale records. A target already
  proven exited is reported separately and is never described as signalled.

Every cleanup candidate gets a per-path result: `removed`, `already-absent`,
`replacement-preserved`, `restore-failed`, `removal-failed`, or another explicit
cleanup failure; a precondition failure reports the candidate as `held`.
Replacement and failure rows identify every preserved location, including any
same-parent holding path. If one alias fails while another succeeds, the
terminal result is `cleanup partial`, not `stopped` or `stale-cleaned`, and the
command fails even though exit and listener release may already be proven. A
signal failure, wait timeout/error, or remaining
target/wildcard/foreign/uninspectable listener is a teardown failure: no
registration is removed and `stopped` is never printed.

Multiple live identities, disagreement between process and listener ownership,
uninspectable state, malformed or unreadable peer state, a live unadopted
schema-v1 record, or any other blocked registration refuses teardown. All
aliases are retained, no process is signalled, and no potentially owning
registration is deleted. Fix the reported state; do not work around it with a
broad process-name kill.

### Recovering a live schema-v1 registration

Historical schema-v1 runfiles remain readable, but a numeric PID alone cannot
authorize teardown. If that PID is live, both `status` and `down` retain every
alias and print a copy/paste-complete recovery command with that numeric PID
only when the originating local record is present. A global-only legacy record
is reported for repair because Ferric cannot safely invent its local origin.
The adoption command has the form `ferric server adopt --pid <pid>`. For
example, if status/down reports PID 6501, run:

```sh
ferric server adopt --pid 6501
```

Adoption is non-destructive. It acquires an exact retained process handle,
checks the closed engine executable and every available recorded argv
coordinate, requires exclusive IPv4-loopback listener ownership, and
conditionally replaces only unchanged local/global aliases with schema v2.
It never signals the process. After replacement, Ferric rechecks that same
retained generation. If the generation changed, an alias was concurrently
replaced, or the transition otherwise fails, Ferric rolls back only the v2 bytes
it wrote that are still unchanged. External replacements remain untouched, and
any failed rollback reports the exact alias and preserved location so no
recovery state is hidden. Any pre-replacement disagreement leaves all original
bytes intact. A later `server down` re-acquires and revalidates the successfully
adopted generation.

If the legacy PID is already absent, run:

```sh
ferric server down
ferric server up --engine llama-server --model /path/to/model.gguf
```

The first command treats the unchanged legacy record as stale and removes it
without signalling a process; the second publishes schema v2. If the PID was
reused or the record is malformed, Ferric continues to refuse automatic
cleanup. In that case, compare the local and global files with the record you
inspected and remove only those exact unchanged files manually.

## Launch flags

`ferric server up` accepts:

| Flag | Default | Purpose |
|---|---|---|
| `--engine <llama-server\|ollama>` | `llama-server` | which engine to launch |
| `--model <PATH\|NAME>` | — | GGUF path (llama-server) or model name (Ollama) |
| `--mmproj <PATH>` | — | multimodal projector GGUF (image/audio/video) |
| `--ctx <N>` | `4096` | context window in tokens |
| `--port <N>` | `8080` | port to bind **on 127.0.0.1** |
| `--threads <N>` | engine default | CPU threads (llama-server only) |
| `--gpu-layers <N>` | engine default | layers to offload to GPU (llama-server only) |
| `--batch-size <N>` | engine default | batch size (llama-server only) |
| `--seed <N>` | engine default | llama.cpp sampling seed; use a non-negative value for reproducibility (`-1` requests a random seed) |
| `--parallel <N>` | engine default | nonzero number of concurrent llama-server request slots |
| `--tailscale` | off | expose the loopback engine at one exactly owned Tailscale Serve path on HTTPS 443; normal operation requires tailscaled capability 142 with version core 1.102.2 on Linux or Windows |

### How the flags map to `llama-server`

Ferric builds a closed, audited command line — it never execs an arbitrary
binary (the engine is a fixed enum). The mapping is:

| Ferric flag | `llama-server` argument |
|---|---|
| `--model` | `-m <path>` |
| `--mmproj` | `--mmproj <path>` |
| `--ctx` | `-c <n>` |
| `--threads` | `-t <n>` |
| `--gpu-layers` | `-ngl <n>` |
| `--batch-size` | `-b <n>` |
| `--seed` | `--seed <n>` |
| `--parallel` | `--parallel <n>` |
| `--port` | `--port <n>` |
| (fixed) | `--host 127.0.0.1` |

For Ollama, `--engine ollama` runs `ollama serve` with `OLLAMA_HOST` set to the
chosen host:port. The edge-resource flags are ignored by that engine, while
`--seed` and `--parallel` are rejected because they describe llama-server
sampling and slot behavior.

## Loopback-only, by design

The host is **pinned to `127.0.0.1`** and is not configurable. The launcher never
binds a public interface. This is ADR-005: the harness and its backend talk only
over loopback, so an inference server Ferric launched is never reachable from off
the machine. This is also why, in the containerized topology, `ferric` and
`llama-server` are co-located in one container rather than split across a network
boundary (see [Container Topology & Roadmap](swarming-k8s.md)).

### Tailscale Serve exposure

`server up --tailscale` leaves the engine bound exclusively to `127.0.0.1` and
adds one path handler to the node's existing Serve configuration through the
local tailscaled API. Ferric asks the operating system for a 128-bit random
token and owns only this coordinate:

```text
https://example-host.tailnet-example.ts.net/_ferric/<32-hex-token>/v1
```

Tailscale strips the mount prefix before proxying, so the local backend still
receives `/v1/...`. The token makes the ownership coordinate unique; it is not
a credential. Tailnet identity, HTTPS, MagicDNS, and ACL policy determine who
can reach the endpoint. Ferric neither owns HTTPS 443 nor the node certificate
nor unrelated Serve handlers.

Normal operations require the response capability header to be exactly 142 and
the Tailscale version's semantic-version core to be exactly 1.102.2; a valid
prerelease or build suffix is accepted. Conventional Linux installations must
expose `/var/run/tailscale/tailscaled.sock`; distributions or appliances that
relocate that socket are not yet supported. Windows connects to Tailscale's
protected named pipe. The invoking account must be permitted to open the local
daemon endpoint. A filesystem or pipe permission failure, or an HTTP 401/403
from LocalAPI, is an authorization problem to correct at the OS/Tailscale
boundary. Preflight failures occur before mutation; a 401/403 received after
POST bytes were sent is indeterminate and retains the journal. macOS is
explicitly unsupported because its sandboxed GUI variants use a different
token-and-port discovery protocol.

Publication binds three things, not just a hostname: the stable node ID, the
canonical dotless FQDN, and HTTPS authority. tailscaled must report
`BackendState=Running`, the self node must advertise the HTTPS capability, and
the FQDN must be present in the certificate domains. Enable Tailscale HTTPS on
the node before launch. Cleanup still requires the same stable node ID, but it
may target the journaled old FQDN after a node rename or later HTTPS-policy
change; a profile or node switch never inherits cleanup authority.

Every authoritative Serve observation uses one LocalAPI connection and reads
`status -> serve-config -> status`. Both status reads must describe the same
stable node, FQDN, backend state, and HTTPS authority. The configuration ETag
must be the lowercase SHA-256 of the exact raw response body. Ferric sends one
whole-document POST with that exact value in `If-Match` and never retries a
mutation. HTTP 412 therefore proves that the stale body was not applied. Any
failure after POST bytes were sent without a no-op response is indeterminate:
the update may have committed, so recovery evidence remains durable.

The Serve-config ETag is not cryptographically bound to the status identity.
The before/after status sandwiches detect and compensate a node or profile
switch, but LocalAPI cannot make that identity check atomic with the POST. In
the narrow race where an operator switches profiles after the pre-POST status
read and the new profile accepts the same configuration ETag, Ferric can detect
the mismatch only afterward, attempt scoped compensation, and retain its
journals. Do not switch Tailscale profiles while `server up --tailscale` or
`server down` is running.

The strict 1.102.2 schema projection is duplicate-key-safe and deliberately
fail-closed. Top-level JSON `null` is the documented empty configuration, but a
null TCP, Web, host, or handler object is invalid. Unknown or wrongly typed
fields on an authority-relevant object, noncanonical ports, conflicting HTTPS
443 state, a true expected-host Funnel setting, effective foreground routing,
and a descendant or trailing-slash alias beneath the owned path all block
publication. These states could widen, shadow, or normalize the exact route;
resolve them explicitly before retrying Ferric.

Launch first proves the path absent on a compatible HTTPS 443 Serve entry,
starts and verifies the exact loopback process, and publishes byte-identical
local/global ownership journals with `apply_confirmed=false`. It then rechecks
the same-session identity and exact configuration revision, applies only that
path to `http://127.0.0.1:<port>` with `If-Match`, and performs a second
same-session identity/configuration sandwich. Only the exact handler on the
same publication identity authorizes compare-replacing every unchanged journal
with `apply_confirmed=true`. The native process/listener and final registration
revisions are revalidated before ready is reported. The local base remains
`http://127.0.0.1:<port>/v1`; the tokenized HTTPS base is reported separately.

`status` reports the external coordinate as active only when the token path
still has the recorded proxy target, the stable node ID and publication FQDN
match, HTTPS authority remains valid, and no effective foreground, Funnel,
descendant, or alias route shadows it. An absent path is pending, a different
handler is replaced, a residual route is shadowed, and unreadable or ambiguous
state is uninspectable. It also reports whether the journal records a confirmed
or unconfirmed apply. Only an exact active proxy together with the existing
Ready native state is success. Each non-ready state prints one safe next action
and the retained journal location.

`down` revalidates the unchanged journal, same-session identity, raw-body ETag,
and exact target, then removes only that handler with one `If-Match` CAS. Every
unrelated JSON value is retained. On the pinned daemon Ferric may also prune an
empty scaffold only when the journal proves Ferric created it and the current
shape still matches exactly. After an upgrade, cleanup has a bounded
best-effort fallback for major-1 versions at or above 1.102.2 and capability at
least 142: it can remove the exact handler while preserving all TCP/Web
scaffolding and unknown fields, but it always retains the ownership journals
because future routing semantics make global endpoint absence unprovable. That
fallback refuses the POST if reserialization could alter an unknown numeric
value. When the original Serve configuration was top-level
`null`, a complete apply/cleanup cycle leaves the semantically equivalent empty
object `{}`; Ferric cannot restore the original lexical `null` without storing
and later replacing unrelated whole-document bytes.

Cleanup then repeats the identity/configuration sandwich, independently
stops/reaps only the exact retained native process, and removes registration
bytes only after both resources resolve. An absent path is idempotent only for
a confirmed journal. A replaced, malformed, uninspectable, or identity-mismatched
handler does not authorize mutation. A descendant/alias or effective foreground
route that remains after exact removal is a residual shadow: Ferric preserves
the ownership journals and reports the manual coordinate instead of claiming
cleanup. An unconfirmed journal also remains when an indeterminate apply is
followed only by absence, until the delayed exact path is observed and removed
or separate daemon-generation/manual proof establishes that it cannot land.

Never use `tailscale serve reset` as Ferric recovery: it would clear unrelated
node-wide state. Ferric has no CLI-command runner for Serve and does not expose
reset, root-path off, or an unscoped mutation. Its only write is the exact
checked LocalAPI configuration body described above. For retained evidence,
inspect the reported token path and target, resolve only that coordinate when
you can independently verify it, then rerun `ferric server down` so Ferric can
converge and conditionally remove unchanged journals. Boolean-only historical
`tailscale: true` records have no typed authority and remain wholly fail-closed
before process or Tailscale effects.

The automated lifecycle acceptance uses a model-free fake LocalAPI. It does not
prove a real tailnet, certificate issuance, MagicDNS, ACL reachability, or live
Windows named-pipe behavior. Perform any live-tailnet acceptance only with
explicit operator authorization. LocalAPI requests are time-, header-, and
body-bounded. On Windows, if cancellation of a pending pipe operation cannot be
observed by the absolute deadline, Ferric closes and poisons the pipe and
retains at most one pending heap allocation for that poisoned session until the
short-lived Ferric process exits; this prevents use-after-free without turning
the timeout into an unbounded wait.

## Edge tuning

`--threads`, `--gpu-layers`, and `--batch-size` exist for constrained targets —
Jetson, Raspberry Pi, and similar. On a CPU-bound edge box, thread count is the
primary latency lever; on a small GPU, `--gpu-layers` decides how much of the
model is offloaded. A representative edge launch:

```sh
ferric server up --engine llama-server --model ./model.gguf \
  --ctx 8192 --threads 4 --gpu-layers 20 --batch-size 256
```

Tune, then **measure** — the [testbench](testbench.md) tells you whether a given
configuration still drives the tools reliably, which is the number that actually
matters for an agent.

## Multimodal

Pass `--mmproj` alongside `--model` to load a vision/audio projector; the model
can then read images and audio when the query declares `--modality`. See
[Multimodal Input](multimodal.md) for the full pipeline.

## Looking ahead: Animus Ophanim

Today the engine is external — llama.cpp via the HTTP valve — and this chapter is
about configuring *that*. The Animus Project's stated direction is a
**native-Rust inference engine, Animus Ophanim**, that will eventually replace
llama.cpp as the default backend and fold the runner into the harness's own
process and language.

> The name is deliberate. The *ophanim* are the wheels of the divine chariot —
> "wheels within wheels," full of eyes, that carry the whole apparatus and move
> wherever the spirit directs. An inference engine is exactly that: the wheels
> that actually move the body, nested computation within computation, all
> attention, going wherever the model's intent points. The spine drives; Ophanim
> is what it drives *with*.

Until Ophanim ships, everything in this chapter is how you get the most out of
the llama.cpp valve. **(Animus Ophanim is stated intent — not yet built.)**

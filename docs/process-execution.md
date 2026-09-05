# Source-driven process verification

Run project code through Cargo. Do not extract test executables, launch files
from `target/` manually, or create background executable proofs to work around
a failing test. Source-defined child modes inside Cargo tests are legitimate:
the source defines their lifetime, ownership, assertions, and cleanup.

For the usual development gate:

```powershell
cargo test --workspace --locked
```

The model-free native lifecycle gate on Windows is:

```powershell
cargo test -p ferric-cli --features lifecycle-fixture --test server_lifecycle_fixture --locked -- --test-threads=1
```

On Linux, use the same source wrapper as CI (requires non-interactive sudo for
namespace setup, plus `unshare`, `setpriv`, and `ip`):

```sh
bash tools/test-lifecycle-linux.sh
```

That wrapper warms the Cargo target, creates an isolated PID/network namespace,
then drops privilege before running `cargo test --offline`. A source shell
remains namespace PID 1 to reap adopted fixture children. It forwards Cargo's
exit status; the namespace supervisor's hard-cleanup link survives credential
changes. It never executes a target artifact directly. The separate namespace
is necessary for the lifecycle suite's complete listener-owner visibility; an
ordinary-host positive ownership claim remains separate work.

## Ownership boundary

`ferric-process` shares the bounded process and file-capture implementation used
by benchmark commands and CLI test adapters. Successful cleanup must prove the
owned scope is empty, not only that its initial child exited. Failure to prove
cleanup cannot produce a successful result. Tests own their cleanup even when
an assertion unwinds; manually killing leftovers does not repair test evidence.
The cleanup deadline rejects final observations made at or after the limit on
both platforms. It is not a hard real-time scheduler or a mechanism to interrupt
an indefinitely stalled native call or mutex acquisition. Last-resort shutdown
is a failure path, never evidence that bounded cleanup succeeded.

Windows uses a kill-on-close Job and suspended creation: ownership precedes
execution, post-create failure rolls back the retained child, and completion
requires both zero active Job processes and signalled retained member handles
within the same cleanup deadline. Accounting alone is not sufficient.
The lifetime admission counter is fenced around the member snapshot and final
drain: an intervening admission makes that snapshot incomplete and cleanup
fails closed. This does not claim retention of every historical process object.

Linux, macOS, and FreeBSD use a cooperative process group; other Unix targets
fail closed before spawning. Native acceptance covers Windows and Linux only,
not macOS/FreeBSD parity. Groups are not a sandbox against descendants that
deliberately change groups. Linux adopted descendants need a scoped reaper;
parent-watch/subreaper installation is an explicit process-wide opt-in for test
harnesses. A watcher cannot clean after its own process has been SIGKILLed.
Controlled cancellation tests therefore require a surviving supervisor/reaper
or namespace boundary. Arbitrary abrupt-owner death and group escape remain
[tracked work](work/tasks.md); they are not accepted platform-parity guarantees.

Capture uses temporary files to avoid full-pipe and inherited-writer deadlocks.
Head/tail limits bound retained memory, not the number of bytes a child can write
to disk. Hostile-output disk quotas and broader process sandboxing are separate
requirements. Existing acquired models and retained run evidence are never
cleanup targets of these model-free gates.

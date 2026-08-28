# Sprint 115 managed-runtime qualification

This directory owns T-11503's single-coordinate, post-reboot qualification and
running-server handoff. It never downloads a model or engine and never launches
a retry, replacement, restart, or fallback qualification attempt. The only
authorized coordinate is the already-acquired Qwen3.8-27B UD-Q4_K_M at 32,768 context and 24 GPU layers,
launched exactly once through the T-11501-qualified Ferric binary.

The single Ferric smoke command retains Ferric's built-in transient provider
backoff because this frozen release exposes no disable switch. This control
does not claim zero underlying HTTP attempts; it proves one qualifier smoke
invocation and no qualifier-level retry or replacement.

Run the static control test first. It parses every PowerShell file, verifies the
frozen control hashes and exact launch contract, and exercises locking and
attempt allocation only in a temporary directory. It does not create a runtime
attempt, start WSL, inspect live server state, or run inference.

```powershell
pwsh -NoLogo -NoProfile -File .\docs\sprints\s115\control-artifacts\runtime\test-runtime-control.ps1
```

Then run the one qualification command from the repository root:

```powershell
pwsh -NoLogo -NoProfile -File .\docs\sprints\s115\control-artifacts\runtime\qualify-runtime.ps1
```

Each invocation acquires an exclusive global control lock and allocates one new
three-digit attempt. Attempts are never reused or removed. Compact evidence is
retained under `attempts/NNN`; transient work and the live, still-growing
llama-server log remain beneath ignored
`target/s115-runtime-qualification/attempts/NNN`.

The qualifier records the complete E17-A host state before enforcing a cold
start. It binds the exact release result, Ferric binary, model, engine binary,
CUDA backend, and 55-file runtime tree; starts Ferric once; proves the two
runfiles, process creation window, exact argv, executable, listener ownership,
health, served model, effective properties, template behavior, constrained
external-trace nonce, and one warmup plus three scored throughput samples. A
successful attempt leaves that same proven process running and publishes a
`qualified_running` handoff. There is no success teardown.

Failures are typed and retained. Cleanup first binds the launched PID to the
pinned executable, exact argv, creation window, runfiles, and sole loopback
listener, then acquires a durable process handle and rechecks its start time,
path, and hash before calling `Kill()` on that process only. Exact-hash runfiles
are removed only after process exit and listener absence are confirmed; a
process whose ownership cannot be proven is not killed speculatively.

Verify retained bytes without contacting the server:

```powershell
pwsh -NoLogo -NoProfile -File .\docs\sprints\s115\control-artifacts\runtime\verify-runtime.ps1 -Attempt latest
pwsh -NoLogo -NoProfile -File .\docs\sprints\s115\control-artifacts\runtime\verify-handoff.ps1 -Attempt latest
```

Re-prove the running handoff before T-11410:

```powershell
pwsh -NoLogo -NoProfile -File .\docs\sprints\s115\control-artifacts\runtime\verify-handoff.ps1 -Attempt latest -CheckLive
```

The live verifier never launches, restarts, stops, or mutates the server. T-11410
must consume the exact endpoint, model identifier, PID, creation time, hashes,
and argv in the handoff. Any mismatch is infrastructure failure; it does not
authorize a second download, coordinate fallback, or unrecorded restart.

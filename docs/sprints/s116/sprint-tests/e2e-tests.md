# Sprint 116 End-to-End Test Results

**Status:** passed on the two runtime platforms exercised.

## Windows

The model-free server lifecycle fixture passed 3/3 tests on Windows. The real
Ferric CLI launched the isolated feature-gated fixture under the ordinary
closed-engine filename, observed its loopback endpoint, exercised lifecycle
commands, and verified cleanup within temporary registration roots.

## Native WSL Linux

The same fixture was built and executed natively inside WSL Linux and passed
3/3 tests. This verifies the Linux pidfd and `/proc` process/listener path on a
native Linux runtime rather than treating a Windows result as Linux evidence.

## Behaviors proved by both runs

- Model-free `server up/status/down` leaves no owned helper process, listener,
  final/staged registration, or unrelated sentinel mutation.
- Tailscale mode refuses before external or registration side effects.
- Legacy adoption is non-destructive, and later teardown requires the adopted
  exact process identity.

The required lifecycle feature fixture also compiled for x86_64 and AArch64.
Only Windows and native WSL Linux x86_64 runtime execution is claimed here;
no AArch64 runtime was available. The optional broader AArch64
all-features/all-targets build was environmentally blocked by the missing
`aarch64-linux-gnu-gcc` tool needed to compile `ring`, which is outside this
sprint's acceptance gate.

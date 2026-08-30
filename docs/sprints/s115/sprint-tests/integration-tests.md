# Sprint 115 integration tests

## Qualified release

T-11501 attempt 002 passed all 20 gates: locked formatting, default and
backend-enabled strict Clippy, targeted query/CLI tests, the whole workspace,
fresh release build, version/help inspection, and four default/external
fresh/resume probes. Offline and live verification reconciled 47 retained
files and all four linked traces.

## Frozen harness and sandbox

T-11502 attempt 004 passed the 30-file plus two generated-input frozen
contract, 295 journal rows, 590 referenced outputs, 156 sandbox invocations,
host/WSL depth probes, standalone external Git rules, and explicit
network-unshare canaries. Offline and live verification reconciled all 92
retained compact evidence files. Four canonical roots were absent at handoff.

## Historical managed-runtime verification

After live attempt 002, the retained verifier was corrected without modifying
the attempt. Offline runtime verification passed all 64 retained files and
accepted only the attempt's exact predecessor control manifest. Offline
handoff verification passed with invariant canonical UTC creation identity.
The static suite proved both attempt snapshots unchanged. A live verification
was correctly not rerun after the server ended.

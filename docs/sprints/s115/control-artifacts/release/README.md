# Sprint 115 release qualification

This directory owns the T-11501 control that binds the Sprint 115 query change
to an exact backend-enabled Windows release binary before any model inference.
Each numbered attempt is fail-once and immutable; a later run allocates the
next number instead of deleting or overwriting an earlier attempt.

Run exactly one command from a normal host PowerShell 7 session at the
repository root:

```powershell
pwsh -NoLogo -NoProfile -File .\docs\sprints\s115\control-artifacts\release\qualify-release.ps1
```

Do not run the qualification in a restricted process sandbox. The full
workspace test gate includes host/process inspection and is expected to need
normal host access. The command does not start a model server or perform model
inference.

## What the command proves

The entrypoint:

- refuses dirty `Cargo.toml`, `Cargo.lock`, or `crates/` state;
- separately reports and tolerates only the known unstaged unrelated edit to
  `docs/sprints/s114/control-artifacts/model/acquisition-tests.json`;
- records locked-dependency format, default/backend strict clippy, targeted
  backend query and CLI tests, full default-feature workspace tests, and the
  exact backend-enabled release build, with an explicit timeout on every gate;
- records every command's argv, working directory, stdout, stderr, exit code,
  start/end time, duration, and stream hashes;
- builds into a fresh per-attempt Cargo target, copies that exact executable to
  `target/release/ferric.exe`, proves byte-count and SHA-256 parity, then binds
  the published binary to its Git commit, version, and `query --help` surface;
- runs four real isolated mock probes: default fresh/resume and external
  fresh/resume. Fresh probes stop at `max_turns`; resume traces are selected by
  set difference and must link through `resumed_from`. External probes must
  leave no workspace `.ferric` state.

The first run uses
`target/s115-release-qualification/attempts/001`; each later run scans the
transient and retained attempt roots and atomically allocates the next numeric
attempt. On complete success, a manifested evidence-only copy is verified
twice and atomically published to the matching retained path (`attempts/001`
for the first run) in this directory. The fresh Cargo target remains transient
and is not copied into retained evidence. Existing attempt contents are never
overwritten, and the script performs no recursive delete. On failure, partial
evidence and the attempt claim remain under that target attempt for diagnosis;
running the same one command again safely allocates the next number.

The retained attempt contains `result.json`, `result.sha256`, `journal.jsonl`,
per-gate stream captures, retained probe traces, and `files.sha256`. Verify it
without rerunning gates:

```powershell
pwsh -NoLogo -NoProfile -File .\docs\sprints\s115\control-artifacts\release\verify-release.ps1 -EvidenceRoot .\docs\sprints\s115\control-artifacts\release\attempts\001
```

The qualifier invokes that verifier with `-CheckLiveBinary` before publication
and again from the publication stage. In that mode it independently compares
the fresh Cargo output, published binary, live probe roots, and retained trace
hashes. The command above is the offline retained-evidence check and does not
depend on ignored transient files still being present.

`test-qualification-control.ps1` is a static/parser self-test. It checks the
dynamic-attempt, fresh-build, exact-publication, gate-capture, and verifier
surface without running Cargo gates, building Ferric, or creating an attempt.

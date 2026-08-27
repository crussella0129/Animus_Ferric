# MH-RS01 command journal schema

The harness writes `target/s114-experiment/app-harness/command-journal.tsv`.
That ignored artifact is an append-only, structured, tamper-evident stream of
run-specific provenance. It intentionally contains run paths and hashes of raw
logs, whose timing text may vary. The tracked scripts, not candidate code, own
it; byte determinism is required of the nine grade records instead.

The first line is a fixed header. Every subsequent line uses schema
`s114-command-journal-v1` and contains these fields in order:

1. schema;
2. one-based sequence number;
3. SHA-256 of the previous entry, or 64 zeroes for the first entry;
4. base64-encoded stage name;
5. base64-encoded working directory;
6. comma-separated, individually base64-encoded argv values;
7. exit code;
8. base64-encoded stdout evidence path;
9. stdout SHA-256;
10. base64-encoded stderr evidence path;
11. stderr SHA-256; and
12. SHA-256 of fields 1 through 11 joined by tab bytes.

Trusted `s114-check-stage-v1` records are retained beside their corresponding
raw stdout log as `.stage.json` files. They are operator evidence, not public
check output; a completed `run-check.sh` publishes only nine ordered grade
records followed by one summary.

There are no timestamps or locale-dependent values in a record. Ordering is
established by the locked sequence counter. A companion `.sha256` file covers
the complete journal after each append. Decoded stdout and stderr paths must
canonicalize beneath a run's `logs/` directory; journal records cannot attest
arbitrary host files.

Model-authored tests use separate `model-tests-compile`, `model-tests-list`,
and `model-tests-run` records. The bounded, read-only list stage proves that
the registered test names themselves cover the seven disclosed topic stems;
source-only test declarations are not treated as runtime registration.

Each sandboxed command record has a sibling Bubblewrap JSON-status artifact in
the same run log directory. The runner requires exactly one positive
child-start record containing mount, network, and PID namespace identifiers
and, unless the outer timeout killed Bubblewrap, exactly one exit record that
matches the trusted driver's exit code before it classifies a nonzero command
as a candidate failure. Bubblewrap owns that file descriptor; candidate code
never receives it. The child-start record attests Bubblewrap's direct `prlimit`
child, not an independent identity check after `prlimit` executes the fixed
command; a missing exit record is accepted only when the outer driver returns
`124` or `137`. Resource limits are applied by trusted `prlimit` after namespace
setup and before the fixed command is executed, so WSL's pre-existing host-user
threads do not consume the candidate's process allowance.

Raw stdout and stderr remain in the journaled operator evidence. For disclosed
model-test, visible/all-target, and CLI failures only, the runner may also emit
a tightly bounded repair excerpt. It samples at most 4,096 bytes from both the
head and tail, selects the first error/panic/assertion context plus its next
line (or the last two lines), and emits at most 160 payload bytes per stream
and stage plus one trusted context marker. It removes ANSI and non-ASCII
control bytes, prefixes every emitted physical line with
`S114-UNTRUSTED` plus a trusted stage and stream label, and always terminates
the line. Ten worst-case failure stages and their bounded log references fit
within the check profile's 12,000-byte stderr limit. Hidden-test and trusted
control diagnostics are never replayed.

An append verifies the companion plus the last record and its immediate chain
link before extending the file. Process initialization verifies the complete
entry chain and companion, without rehashing historical output artifacts, then
records the tail as a checkpoint. `run-check.sh` finalization verifies the
chain and output hashes added since its checkpoint. The top-level
`self-test.sh` provenance finalization performs the full historical check:
chain, companion, canonical evidence paths, and every referenced output hash.
While holding the journal lock, it then retains the verified journal and its
companion in a content-addressed `evidence/journal-snapshot-<sha256>/`
directory. The tracked summary references those immutable snapshot bytes,
rather than the live journal that later checks may extend. That snapshot does
not copy the referenced raw logs or their sibling stage and Bubblewrap-status
records; the sprint closeout evidence manifest must retain and hash those
artifacts for self-contained long-term review.

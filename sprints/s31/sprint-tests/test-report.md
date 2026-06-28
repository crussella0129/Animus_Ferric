# Sprint 31 Test Report — Ornstein increment 2 (`Retriever` + Local-FS, ADR-041)

**Date:** 2026-06-27. Built the keystone `Retriever` trait + the first source plane (Local FS)
+ the `research()` pipeline. The first real source now plugs into the s30 quarantine
end-to-end. All tests green.

## Build / Lint (green)
- `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean;
  `cargo fmt --all --check` clean. `async-trait` + `tempfile` added to `ferric-research`.

## Unit — `ferric-research::retriever` (temp dir + MockProvider, deterministic) — 7/7 pass
- `matches_by_content_and_excludes_non_matches`: a file containing the query → one
  `RetrievedChunk` (`source` = relpath, `content` = file text); the unrelated file excluded.
- `matches_by_name_and_is_case_insensitive`: query in the *filename* (not body) matches; `TAILSCALE` matches `tailscale`.
- `skips_noise_dirs_and_binary_files`: a `.git/` match and a non-UTF-8 file are both skipped; only the plain text file is returned.
- `respects_max_files_cap`: 5 matching files, `max_files=2` → exactly 2 chunks.
- `availability_and_plane`: `available()` true on a real dir, false on a missing root; `plane() == "local"`.
- **`research_pipeline_source_to_quarantined_digest`** — the headline: a real file on disk +
  a MockProvider → `research()` returns **one** `ResearchDigest` with `untrusted == true` and
  `source ==` the file's relpath. Proves **source → quarantine → provenance-tagged digest**.
- `research_on_unavailable_retriever_is_empty`: an unavailable plane → `Ok(vec![])` (capability-probed no-op).

(Plus the 4 inc-1 quarantine tests still pass: 11 total in the crate.)

## Verdict
**Ornstein increment 2 validated.** The `Retriever` keystone (user-reviewed at the plan
checkpoint) is in place, async-ready for the network planes, and the Local-FS plane drives the
full source→funnel→digest pipeline deterministically. The design invariant holds: even local
content is untrusted and routes through the quarantine, with the retriever adding root +
no-symlink-follow confinement on top. Next (user build order): the Tailnet/NAS-FS retriever over
Tailscale (same trait, reached over the network), then Web+container. No live dependency this
increment; no human checkpoint. ADR-041.

Finalized - DO NOT EDIT

# Sprint 31 Test Plan — Ornstein increment 2 (`Retriever` + Local-FS)

## Unit — `retriever.rs` (`ferric-research`; temp dir + MockProvider, deterministic)
- **content match:** a temp tree with `notes.md` containing "tailscale" + an unrelated file →
  `LocalFsRetriever::retrieve("tailscale")` returns one `RetrievedChunk` whose `source` is
  `notes.md` and `content` is the file text; the unrelated file is excluded.
- **name match:** a file `tailscale-setup.txt` (query in the *name*, not the body) → matched.
- **case-insensitive:** query `TAILSCALE` matches content/name `tailscale`.
- **skips:** a `.git/` (noise) dir entry and a binary (non-UTF-8) file are both skipped; entries
  walked in sorted order.
- **caps:** `max_files` bounds the number of chunks; oversized files are byte-capped.
- **availability:** `available()` is `false` for a non-existent root, `true` for a real dir;
  `plane() == "local"`.

## Unit — `research()` pipeline (`ferric-research`; MockProvider)
- **end-to-end (the headline):** a temp file containing the query + a MockProvider scripted with
  a valid digest JSON → `research(&local_retriever, &mock, query)` returns **one**
  `ResearchDigest` with `untrusted == true` and `source ==` the file's relpath. Proves
  **source → quarantine → provenance-tagged digest**: a real file on disk becomes typed data with
  no channel to act.
- **unavailable → empty:** `research()` with a retriever whose root doesn't exist → `Ok(vec![])`
  (capability-probed, not an error).

## Build / Lint (default CI)
- `cargo test --workspace` green; `clippy --workspace --all-targets -- -D warnings` clean;
  `fmt --check` clean. `async-trait` added to `ferric-research` (already a workspace dep).

## E2E
- Not required: the Local-FS plane is fully deterministic (temp dir) and the quarantine is
  MockProvider-covered. The live web/tailnet planes (inc 3/4) carry their own E2E (real network /
  a tailnet device).

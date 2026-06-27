# Sprint 29 Meta

- **Sprint number:** 29
- **Start timestamp:** 2026-06-27T20:42:37Z
- **End timestamp:** 2026-06-27T21:10:00Z
- **Model:** claude-opus-4-8
- **Exit status:** success
- **Token count:** (not observed)
- **Summary:** Rounded out **Ring 2** with `apply_patch` (the rings-memory "room to grow"; pivot from the now-complete loop-guard family back to the tool rings, the north star). `apply_patch` (`crates/ferric-tools/src/builtin/apply_patch.rs`, ring 2, Write) applies a context-located unified diff to one file, atomically: `@@`-delimited hunks with ` `/`-`/`+ lines (line numbers ignored — matched by context), line-based split/splice/join (round-trips the trailing newline), single atomic write (failure = byte-identical). Distinct from `multi_edit`: a hunk's context **disambiguates** which occurrence to edit (multi_edit's `replacen` hits only the first) + diff-format familiarity. Registered; `rings_gate` Medium 11→12 (Ring 2 = multi_edit + apply_patch), Nano 6 / Small 10 unchanged. 5 tests incl. the contrast that edits the 2nd of two identical lines. Single-file scope (multi-file deferred). No registry/scale change. ADR-039; README Status 29. One PR per sprint; `dev` clean (PR #14 merged).

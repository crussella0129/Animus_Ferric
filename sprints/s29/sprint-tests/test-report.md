# Sprint 29 Test Report — `apply_patch` rounds out Ring 2 (ADR-039)

**Date:** 2026-06-27. Pivoted from the (now-complete) guard family back to the tool rings.
Shipped `apply_patch`, the second Ring-2 tool — a context-located, atomic unified-diff
applier whose context disambiguates which occurrence to edit. All tests green.

## Build / Lint (green)
- `cargo test --workspace` green (all crates); `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --all --check` clean.

## Integration — `builtin_file_tools.rs` (`ferric-tools`, via the registry chokepoint) — 5/5 pass
- `apply_patch_single_hunk_applies`: `@@\n a\n-b\n+B\n c` on `a\nb\nc\n` → `a\nB\nc\n` (exact content; trailing newline preserved).
- **`apply_patch_context_disambiguates_the_second_occurrence`** — the proof: a file `x\nkeep\nx\n`; a hunk with context `keep` then `-x\n+X` edits the **second** `x` → `x\nkeep\nX\n`. `multi_edit`'s first-occurrence rule would have hit the first — this is the capability it lacks.
- `apply_patch_absent_context_is_error_and_no_write`: a hunk whose `-` line isn't in the file → error **and** the file is byte-identical (no partial write).
- `apply_patch_malformed_or_empty_is_error`: an empty patch and a body line without a ` `/`-`/`+ prefix both error; the file is untouched.
- `apply_patch_multi_hunk_applies_in_order`: two hunks in one patch both apply → `one\nTWO\nthree\nFOUR\n`.

## Ring-gate — `rings_gate_builtins_by_tier` — pass
- **Medium == 12** (Ring 0 `6` + Ring 1 `4` + `multi_edit` + `apply_patch`); both Ring-2 tools present.
- **Nano still 6**, **Small still 10** — `apply_patch` (ring 2) correctly absent below Medium. No registry/scale change (Medium `max_tools=16` ≥ 12).

## Verdict
**`apply_patch` validated.** The contrast test demonstrates the concrete capability over
`multi_edit`: context-disambiguated editing (target the Nth occurrence). The tool is atomic
(failure → byte-identical file), line-based (round-trips newlines), and ring-gated to
Medium+. Ring 2 is now a 2-tool set, advancing the rings north star. Single-file scope;
multi-file `apply_patch` is the noted follow-on. No human checkpoint (a pure-`std::fs`
builtin fully covered through the registry, like its siblings). ADR-039.

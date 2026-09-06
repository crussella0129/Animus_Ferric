# Sprint 122 Unit Tests

Tested head: `9eabcbc` (T-12203 evidence commit). Runner: `cargo test -p ferric-core -p ferric-cli --bin ferric -- --test-threads=1`. Result: **403 passed / 0 failed** (ferric-cli, up 7) plus **4 passed / 0 failed** (ferric-core fit). `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets --locked -- -D warnings` clean.

## T-12201 — memory probe (`crates/ferric-cli/src/startup/memory.rs`)
- `parse_meminfo_reads_total_and_available` — standard `/proc/meminfo` → `total_bytes`/`available_bytes` in bytes (kB×1024). **PASS** (EARS: WHEN standard meminfo THEN total+available).
- `parse_meminfo_missing_available_is_none` — text with `MemTotal` but no `MemAvailable` → `None`. **PASS** (EARS: WHEN no MemAvailable THEN None, no fabrication).

## T-12202 — pure fit model (`crates/ferric-core/src/fit.rs`)
- `estimate_covers_weights_and_context` — `estimate(f,0) > f`, monotonic in context, saturates on `u64::MAX`. **PASS** (EARS: WHEN estimate THEN ≥ weights + positive allowance).
- `classify_fits_tight_and_wontfit` — 1/8 GiB → Fits, 7/8 → Tight, 9/8 → WontFit. **PASS** (EARS: the three bands).
- `classify_none_is_unknown` — `available = None` → Unknown. **PASS** (EARS: WHEN None THEN Unknown).
- `human_test_27b_on_small_ram_is_wontfit` — 15.3 GiB vs 8 GiB available → WontFit. **PASS** (EARS: the human-test case).

## T-12203 — front-door surfaces (`crates/ferric-cli/src/human.rs`)
- `picker_annotates_each_model_fit` — a fitting model annotates "fits"; a 20 GiB model annotates "won't fit" and names the free memory. **PASS** (EARS: WHEN picker + known memory THEN annotate each).
- `wontfit_requires_extra_confirmation` — a WontFit selection yields a confirmation prompt containing "won't fit", "free", and "[y/N]"; a comfortable model yields `None`. **PASS** (EARS: WHEN WontFit THEN extra confirmation naming numbers).
- `unknown_memory_preserves_current_behavior` — `None` memory → empty annotation, no confirmation prompt, and the verbatim "Resource fit is not measured." start line. **PASS** (EARS: WHEN Unknown THEN preserve current behavior).
- `fit_keys_on_available_not_total` — 64 GiB total / 2 GiB available with a 10 GiB model → WontFit (critique C-002). **PASS**.

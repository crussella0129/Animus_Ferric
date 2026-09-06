# Plan Critique — Sprint 122

## Concerns

### C-001: Promoting `windows-sys` from dev-dep to dep must preserve existing features
- **Where:** `build-plan.md` T-12201 / **Touches** `crates/ferric-cli/Cargo.toml`
- **Quote:** "Promote `windows-sys` from dev-dep to a `cfg(windows)` dep + `Win32_System_SystemInformation`"
- **Failure mode:** hidden-dep
- **Why it matters:** The existing `[target.'cfg(windows)'.dev-dependencies] windows-sys` enables `Win32_Foundation`, `Win32_Security`, `Win32_System_Diagnostics_ToolHelp`, `Win32_System_JobObjects`, `Win32_System_Threading` for the process-containment tests. If the promoted regular dependency does not carry the **union** of those plus `Win32_System_SystemInformation`, the Windows test build breaks — a red CI on the exact platform this sprint targets.
- **Suggested response:** fix-in-plan. T-12201 builds the promoted `[target.'cfg(windows)'.dependencies] windows-sys` with the union of the current dev-dep features **plus** `Win32_System_SystemInformation`, and removes the now-redundant dev-dep entry (a regular dep is visible to tests). A green `cargo test -p ferric-cli` on Windows is the check.

### C-002: `classify_fit` must key on available, not total, memory
- **Where:** `build-plan.md` T-12202 / research "available vs total" risk
- **Quote:** "Use available for the warning threshold, disclose the number, and keep the decision the user's."
- **Failure mode:** missing-risk
- **Why it matters:** A fit computed against *total* RAM would call a model "Fits" while the OS and other apps already hold most of it — re-introducing a softer version of the human-test trap. The signature takes `available`, but nothing tests that total is not used.
- **Suggested response:** fix-in-plan. `classify_fits_tight_and_wontfit` includes a case where `total` is large but `available` is small and asserts `WontFit`/`Tight` — pinning the classifier to available.

## Confidence
proceed-with-caveats

Both concerns are `fix-in-plan` build/test details, not intent, coverage, or
dependency errors that locking would commit to incorrectly. C-001 is folded into
T-12201's Cargo work; C-002 strengthens the T-12202 classify test. No acceptance
criterion is weakened; AC-13's acquisition and GPU/VRAM clauses remain explicitly
active follow-on work rather than silently dropped.

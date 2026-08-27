# Sprint 39 Integration Tests

Black-box subprocess tests against the real `ferric` binary (`crates/ferric-cli/tests/cli.rs`),
plus `ferric-loop`'s in-process `run()`/`replay()` integration tests (see `unit-tests.md`'s T-3904
section — categorized there since they exercise `ferric-loop` directly, not the CLI).

## T-3905 — `ferric query --resume <path>`
All six use a hand-written, `ferric-loop::replay`-shaped trace fixture (`write_interrupted_trace_
fixture`: `SessionPrompt` + `PolicySelected` (`NativeTools`, matching `--mock`'s resolved protocol)
+ one completed turn — a `write_file` call+result — no `SessionEnd`), run through the REAL `ferric`
binary:
- `resume_continues_an_interrupted_session` — `--resume <path>`, no prompt; succeeds; the NEW
  trace's `SessionStart.resumed_from` names the original session id.
- `resume_with_extra_prompt_appends_nudge` — `--resume <path> "extra instruction"` vs. the same
  fixture with no extra prompt: asserts the assembled prompt's char count grows by EXACTLY
  `len("extra instruction")` (test-critic C-002: an earlier `>=` floor passed even when the extra
  prompt was silently dropped, since the replayed history alone already cleared it).
- `no_resume_and_no_prompt_is_a_usage_error` — neither given → clap usage error (regression: proves
  today's "prompt required" behavior is unchanged when `--resume` is never used).
- `resume_protocol_mismatch_is_a_clear_error` — the fixture records `NativeTools`; `--protocol
  grammar` forces a `ConstrainedJson` resolution → fails, naming both protocols, no run attempted.
- `resume_already_stopped_is_a_clear_error` — a fixture WITH a `SessionEnd` → fails, naming the
  original stop reason (`final_text`), no run attempted.
- `resume_with_animus_md_prints_ignored_note` (C-009) — an `Animus.md` present + `--resume` →
  stderr carries the "ignores --prompts-dir/Animus.md" note. Extended (test-critic C-003) with the
  negative-path sibling assertion in `animus_md_present_traces_note` (no `--resume`): stderr does
  NOT carry that note, proving it's genuinely resume-gated rather than unconditional.

## Result
`cargo test -p ferric-cli --test cli`: 27 passed (up from 21). `cargo test --workspace`: all green.
`cargo clippy --workspace --all-targets` clean on default, `backend-openai`, and `backend-mistralrs`
feature sets. `cargo fmt --all --check` clean. See `critique.md` for the test-critic pass and fixes.

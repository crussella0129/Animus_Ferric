# Sprint 110 Research Report — Demo Readiness

## Objective

Review and refactor Animus Ferric into a state that can be demonstrated
reliably on Monday, prioritizing safe user-visible behavior over feature count.

## Baseline

- Branch: `dev` at `c26ee000`; `origin/main..dev` contains only the current
  mdBook commit.
- Working tree before this sprint: only the owner's untracked `AGENTS.md`.
- `cargo fmt --check`: clean.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test --workspace`: 595 passed, 2 Windows-ignored, 0 failed.
- `cargo clippy -p ferric-cli --features backend-openai --all-targets -- -D warnings`: clean.
- `cargo check --workspace --target aarch64-unknown-linux-gnu`: clean.
- Release build with `backend-openai`: clean.

The green suite establishes a good implementation baseline, but not demo
readiness. Several defects live across crate/surface boundaries that the
existing tests do not exercise.

## Findings

### Critical boundaries

1. `ferric trace verify` replays recorded tool calls through the full builtin
   registry against a workspace path controlled by the trace. A verifier can
   therefore write, delete, run commands, or fall back to the current
   directory. Its comparison checks only event count and variant, so it is
   simultaneously dangerous and too weak.
2. MCP `files` values reach the shared attachment router as arbitrary host
   paths. The router reads them with `std::fs` without `Workspace::resolve` or
   the sensitive-read / `.ferricignore` policy.
3. `shell_exec` treats `current_dir(workspace)` as containment. A shell can
   name absolute paths or `..`; the six-pattern denylist is a footgun catcher,
   not an OS boundary. The model currently receives this tool at Ring 2.
4. Foreground `shell_exec` discards nonzero exit status, so failed commands are
   fed back as successful tool results and failure guards cannot engage.

### Demo-path drift

1. Streaming is now default-on and the CLI exposes `--no-stream`, while README
   and demo/reference docs still instruct users to pass removed `--stream`.
2. The demo's `Animus.md` check greps terminal output, but query records the
   application notice only in the trace.
3. `workspace/run-e2e-sweep.sh` still passes removed `--backend openai`.
4. Both live E2E runners can print completion and exit zero without checking
   the native command status and expected artifact.
5. Some docs say `.ferric/traces`; every writer uses `.ferric/trace`.
6. The installed `ferric` on PATH predates the fresh release binary; the demo
   must use/reinstall the binary built from this sprint.

### Outcome semantics

The CLI and MCP treat every loop stop other than `ProviderError` as success.
Budget exhaustion, guard stops, malformed/truncated action, interrupt, and hook
failure therefore exit/report success even though the requested work is
incomplete.

### Environment limitations

- Docker is currently unavailable. Docker/gVisor behavior cannot be claimed
  from this machine; several live sandbox tests skip when no daemon exists.
- A stale, unreachable server registration exists locally. The Monday path
  should preflight state and prefer the deterministic offline demonstration
  unless a real server/model is deliberately prepared.

## Scope decision

For Monday, the trustworthy story is the constrained, traceable file-tool
loop at Ring 0 plus offline skills/ICM/cron demonstrations. The sprint will:

- make trace verification side-effect free;
- route attachments through the workspace read guard and size bounds;
- remove unsandboxed shell/task execution from every model tier while
  retaining explicit human passthrough;
- make process failures and incomplete loop stops report failure;
- replace false-green demo checks with one deterministic smoke runner; and
- align the docs with the actual CLI.

OS sandboxing, remote API authentication, Tailscale lifecycle hardening, and
full live-model/server identity work remain follow-ups. They will not be
represented as validated in the Monday runbook.

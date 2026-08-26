# Sprint 105 build plan — Finalized - DO NOT EDIT

**Principle:** apply sprint 97's lesson (a tracked machine-specific file
overrides the portable default on every checkout) everywhere it holds, not just
to `docker/.env`.

## T-10501 — untrack what the tooling already calls ephemeral

`git rm --cached` only — **files stay on disk, and stay in history**.

- `sprints/` (139 files, already in `.gitignore`; the ignore rule never applied
  because it was added after the files were tracked).
- `scratch/` — add to `.gitignore`.
- `benchmarks/results.jsonl` — this machine's run log; gitignore it.

## T-10502 — machine identity out of Rust fixtures

Keep every fixture's **shape** (that is what the tests assert) and replace the
identity with documentation-range values:

- `ferric-cli/src/server.rs` — `100.86.207.71` → `100.64.0.1`
  (RFC 6598 / the CGNAT range Tailscale actually uses, so the fixture stays
  realistic), `tail944782.ts.net` → `tailnet-example.ts.net`, `TEC-XX` →
  `EXAMPLE-HOST`, node ID → a placeholder.
- `ferric-research/src/retriever.rs` — device names → `example-linux` /
  `example-phone`; IPs → `100.64.0.x`; **drop the account handle**.
- `animus-launch/src/lib.rs:202` — cite the *shape* of the problem, not one
  machine's file.

Comments saying "captured from this machine" become "shaped like real output",
which is the honest claim once the values are synthetic.

## T-10503 — defaults nobody else can satisfy

- `tools/run_benchmarks.ps1` — drop the `D:\Models\gguf\…` default; require the
  path and say so.
- `docker/.env.example` — teach the shape, not one drive letter.
- `benchmarks/model_profiles.json` → `model_profiles.example.json`, gitignore
  the real one. **This is behavioural, not cosmetic:** ADR-029 reads a stored
  `measured_level` back and overrides the params prior in both directions, so a
  shipped profile means a fresh user inherits a tier measured on another
  machine at an unpinned quantization. Verify the read-back still no-ops
  cleanly when the file is absent (ADR-029 says it should — check, don't
  assume).
- `docker-compose.yml`'s `../../Animus/Models` default is a suite-layout
  assumption rather than a machine path: **kept**, with a comment saying so.

## T-10504 — record

ADR-096; README template section; surface the history/privacy consequence as a
decision for the user rather than burying it.

## Not done

`decisions.md` / `agent-tasks/` / git history are not scrubbed — see the
research report. Rewriting the ADRs to remove the machine that produced the
measurements would falsify the evidence they cite, and a history rewrite is a
destructive force-push on a shared branch: the owner's call.

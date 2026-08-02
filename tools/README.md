# tools/

Test and benchmark runners. `demo-smoke.ps1` is the deterministic, offline
pre-demo gate; the other runners drive Ferric against a real model or Docker
daemon and cover behavior `cargo test` deliberately does not. Run them from the
repository root; report paths are relative to the CWD.

| Script | Shell | What it does |
| --- | --- | --- |
| `demo-smoke.ps1` | PowerShell | Builds the release binary and exercises the Monday-safe offline query, trace, guard, skills, launch, ICM, and cron path. |
| `run-e2e.sh` | bash | Containerized deterministic mock query with artifact and trace assertions. |
| `run-coverage.sh` | bash | Coverage over the workspace. |
| `e2e_test.ps1` | PowerShell | Single query end-to-end, then inspects the emitted trace. |
| `run_benchmarks.ps1` | PowerShell | The `ferric bench` sweeps — toolbench, fleet, ring calibration, L0–L6. Writes `toolbench_*.md` and updates `benchmarks/model_profiles.json`. |

Both dialects are present because the benchmark and trace runners were written
on Windows and the coverage and e2e runners under WSL. That is a real split, not
an oversight to tidy away: rewriting either side would mean re-validating it
against a live model, which costs more than the inconsistency does.

## What is deliberately not here

`workspace/run-e2e-sweep.sh` stays in `workspace/`. That directory is **mounted
at `/workspace` inside the container**, alongside `test-sweep-prompt.txt` and the
`.ferric` state the sweep asserts against; the script `cd`s to `/workspace` and
reads those by path. Moving it here would put it outside the mount it operates
on. The sprint-82 audit counted it as scattering — it is placement.

`run-tool-sweep.ps1` was deleted rather than moved (sprint 96). It was a
one-line `cargo run` at the repo root that read `test-sweep-prompt.txt`
relative to the CWD, but that file only exists in `workspace/` — so it could
not run as placed, and `run-e2e-sweep.sh` already does the same sweep properly
from inside the container.

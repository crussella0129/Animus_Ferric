# Sprint 112 Review

## Outcome

Complete. The sprint delivered a real-server-only internal autonomy baseline,
durable continuation semantics, structured ambiguity handling, named
verification gates, a discriminating 24-task corpus, and hardened server launch
readiness.

## What held up under review

- Exact Cartesian accounting keeps missing/duplicate/category-drift rows from
  inflating results.
- Infrastructure failures are separate from contract/objective failures.
- Recovery traces bind workspace, protocol, policy, turn budget, predecessor,
  and retained SHA-256 evidence.
- The model cannot choose grader argv; passing evidence must follow the latest
  mutation.
- Every seed repository fails its grader, and the final independent audit found
  no remaining prompt/grader contradiction or whole-function coverage hole.
- Live server auto-discovery worked without `--api-base`; three hardened cold
  launches proved the final release path.

## What did not work

- The targeted Qwen sample passed only 3/6 objectives. Resume transport worked
  in R08, but the model still failed the final objective.
- H01/recovery asked for an `old_string` instead of reading the files, then
  stopped unnecessarily.
- H01/repository-brief consumed 26 turns and 670.6 seconds without passing; a
  metadata-only tree did not create reliable long-horizon behavior.
- Corpus-level duplicate/collateral effects are not instrumented, so v1 emits
  null and makes no claim.
- `server down` does not yet bind a runfile PID to executable/start identity.

## Claim boundary

The six live rows establish that the measurement and recovery machinery works
and that this 7B model can complete some constrained repository tasks. They do
not establish a population accuracy, pass³ reliability, external benchmark
standing, or a reliable general autonomous coding agent. Full 72/216 collection
is Sprint 113 work.

## Recommended next loop

Collect the frozen baseline first. Then use held failures to test read-before-
edit recovery, compact verification-guided repair state, and better long-horizon
progress summaries. Accept changes only on cross-task held coordinates with
unsafe-edit and unnecessary-question rates visible.

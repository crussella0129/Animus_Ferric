# Sprint 126 pilot — native vs valve on the 27B

Real Hermes (Animus Amalgam `f820dbfd76`) ran the same four tasks through two arms on one llama.cpp process:

- **native** — Hermes's ordinary request with native `tools`; the valve in **record-only** mode forwards it byte-for-byte.
- **valve** — the same request through Ferric's constrained valve, where one harness-authored JSON-Schema action is enforced by llama.cpp (the `thought`, `tool`, `args` grammar, today's defaults).

Both arms pass through the same valve process and receipt code, so the constraint transform is the only difference. Arm order is counterbalanced (A B | B A | A B). Every session starts from an erased slot.

**This is a three-repetition pilot. It estimates feasibility and variance; it supports no advancement or general-capability claim.** The arm-B advancement decision belongs to Amalgam (INT-0004, T-202).

## Manifest

| Coordinate | Value |
|---|---|
| Model | `"Qwen3.8-27B-UD-Q4_K_M.gguf"` |
| Model SHA-256 | `"322e194ff79741c7baa497c240f677f54b201b0efab44ca8e50f122b39123482"` |
| llama-server SHA-256 | `"aa2e1f5c67be55f11be26ae58d643a545ca07c6de498f6f870330ac2f4dfec73"` |
| llama.cpp build | `"b10964-b29c606e2"` |
| Server argv | `["-m","<model>","-c","16384","-ngl","24","-fa","on","-ctk","q8_0","-ctv","q8_0","-np","1","--jinja","--cache-ram","0","--host","127.0.0.1","--port","8181","--no-webui"]` |
| Context / GPU layers / model layers | `16384 / 24 / 66` |
| Output cap / Hermes max turns | `512 / 12` |
| Measured prefill / decode (tok/s) | `276.2 / 3.25` |
| Enforcement probe | `{"Ok":"enforced"}` |
| Derived session deadlines (s) | `{"create":3900.926302488,"edit":2600.617534992,"lookup":1950.463151244,"no_tool":650.154383748}` |
| Deadline margin | `3.0` |
| Repetitions | `3` |
| Ferric commit | `"f06b57ca52442a1a3f9eb188950b4f1dddf53ff8"` |
| Amalgam (Hermes) commit | `"f820dbfd76d002883cfa98cb92352921574a31f1"` |
| Corpus SHA-256 (`e2e/tasks.json`) | `"6db2c6717213da5110f09e09024f666d63dbda10bd92b45238458ee678d1e61a"` |
| Host | `{"os":"windows","arch":"x86_64","logical_cpus":24,"total_memory_bytes":34281484288,"available_memory_bytes_at_start":17997369344}` |
| Runs merged (continuations resume the same schedule) | `[{"run":"pilot-1790430708089","start_index":null,"rates":{"prefill_tokens_per_second":276.1691140579556,"decode_tokens_per_second":3.2530209891013633},"ferric_commit":"f06b57ca52442a1a3f9eb188950b4f1dddf53ff8","sessions":24},{"run":"pilot-1790436617596","start_index":null,"rates":{"prefill_tokens_per_second":272.35111193086436,"decode_tokens_per_second":3.262065360372593},"ferric_commit":"f06b57ca52442a1a3f9eb188950b4f1dddf53ff8","sessions":12}]` |

## Per-arm results (24 included sessions; 12 excluded, listed below)

| Metric | native | valve |
|---|---|---|
| Sessions | 12 | 12 |
| Independently checked completions | 12 / 12 | 12 / 12 |
| Tool calls (sessions with all calls valid) | 24 (12/12) | 27 (12/12) |
| Model requests | 30 | 39 |
| Truncated requests (`length`) | 0 | 0 |
| Requests not completed (failed or cancelled) | 0 | 0 |
| Sessions stopped by the derived deadline | 0 | 0 |
| Sessions with a driver error | 0 | 0 |
| Check infrastructure errors | 0 | 0 |
| Decoded tokens per request | 31.9 | 42.3 |
| Decoded tokens per checked completion | 79.8 | 137.5 |
| Decode time per session (s) | 22.6 | 39.1 |
| Wall time per session (s) | 53.1 | 69.7 |
| Wall time per checked completion (s) | 53.1 | 69.7 |
| Prompt tokens served from cache | 58.1% | 67.4% |
| Sessions whose rendered prefixes only grew | 12/12 | 12/12 |
| Sessions with incomplete upstream metrics | 0 | 0 |

## Per task

| Task | Arm | Completed | Mean wall (s) | Mean decoded tokens | Mean requests |
|---|---|---|---|---|---|
| create | native | 3/3 | 81.5 | 193.0 | 4.0 |
| create | valve | 3/3 | 112.8 | 276.7 | 6.7 |
| edit | native | 3/3 | 53.0 | 91.0 | 3.0 |
| edit | valve | 3/3 | 65.8 | 145.7 | 3.3 |
| lookup | native | 3/3 | 49.0 | 32.0 | 2.0 |
| lookup | valve | 3/3 | 60.3 | 77.0 | 2.0 |
| no_tool | native | 3/3 | 28.7 | 3.0 | 1.0 |
| no_tool | valve | 3/3 | 40.0 | 50.7 | 1.0 |

## Excluded sessions

| Run | # | Task | Arm | Rep | Checker said | Reason |
|---|---|---|---|---|---|---|
| pilot-1790430708089 | 2 | edit | native | 1 | true | infrastructure: the runner gave Hermes a Windows verbatim-path working directory, so every file write failed with mkdir '//?' in both arms and completions depended on the model guessing absolute paths. Runner defect, fixed by strip_verbatim; both tasks were re-run on the fixed runner in pilot-1790436617596. |
| pilot-1790430708089 | 3 | edit | valve | 1 | true | infrastructure: the runner gave Hermes a Windows verbatim-path working directory, so every file write failed with mkdir '//?' in both arms and completions depended on the model guessing absolute paths. Runner defect, fixed by strip_verbatim; both tasks were re-run on the fixed runner in pilot-1790436617596. |
| pilot-1790430708089 | 4 | create | native | 1 | true | infrastructure: the runner gave Hermes a Windows verbatim-path working directory, so every file write failed with mkdir '//?' in both arms and completions depended on the model guessing absolute paths. Runner defect, fixed by strip_verbatim; both tasks were re-run on the fixed runner in pilot-1790436617596. |
| pilot-1790430708089 | 5 | create | valve | 1 | false | infrastructure: the runner gave Hermes a Windows verbatim-path working directory, so every file write failed with mkdir '//?' in both arms and completions depended on the model guessing absolute paths. Runner defect, fixed by strip_verbatim; both tasks were re-run on the fixed runner in pilot-1790436617596. |
| pilot-1790430708089 | 10 | edit | valve | 2 | false | infrastructure: the runner gave Hermes a Windows verbatim-path working directory, so every file write failed with mkdir '//?' in both arms and completions depended on the model guessing absolute paths. Runner defect, fixed by strip_verbatim; both tasks were re-run on the fixed runner in pilot-1790436617596. |
| pilot-1790430708089 | 11 | edit | native | 2 | false | infrastructure: the runner gave Hermes a Windows verbatim-path working directory, so every file write failed with mkdir '//?' in both arms and completions depended on the model guessing absolute paths. Runner defect, fixed by strip_verbatim; both tasks were re-run on the fixed runner in pilot-1790436617596. |
| pilot-1790430708089 | 12 | create | valve | 2 | false | infrastructure: the runner gave Hermes a Windows verbatim-path working directory, so every file write failed with mkdir '//?' in both arms and completions depended on the model guessing absolute paths. Runner defect, fixed by strip_verbatim; both tasks were re-run on the fixed runner in pilot-1790436617596. |
| pilot-1790430708089 | 13 | create | native | 2 | true | infrastructure: the runner gave Hermes a Windows verbatim-path working directory, so every file write failed with mkdir '//?' in both arms and completions depended on the model guessing absolute paths. Runner defect, fixed by strip_verbatim; both tasks were re-run on the fixed runner in pilot-1790436617596. |
| pilot-1790430708089 | 18 | edit | native | 3 | true | infrastructure: the runner gave Hermes a Windows verbatim-path working directory, so every file write failed with mkdir '//?' in both arms and completions depended on the model guessing absolute paths. Runner defect, fixed by strip_verbatim; both tasks were re-run on the fixed runner in pilot-1790436617596. |
| pilot-1790430708089 | 19 | edit | valve | 3 | false | infrastructure: the runner gave Hermes a Windows verbatim-path working directory, so every file write failed with mkdir '//?' in both arms and completions depended on the model guessing absolute paths. Runner defect, fixed by strip_verbatim; both tasks were re-run on the fixed runner in pilot-1790436617596. |
| pilot-1790430708089 | 20 | create | native | 3 | true | infrastructure: the runner gave Hermes a Windows verbatim-path working directory, so every file write failed with mkdir '//?' in both arms and completions depended on the model guessing absolute paths. Runner defect, fixed by strip_verbatim; both tasks were re-run on the fixed runner in pilot-1790436617596. |
| pilot-1790430708089 | 21 | create | valve | 3 | false | infrastructure: the runner gave Hermes a Windows verbatim-path working directory, so every file write failed with mkdir '//?' in both arms and completions depended on the model guessing absolute paths. Runner defect, fixed by strip_verbatim; both tasks were re-run on the fixed runner in pilot-1790436617596. |

## Session ledger (every session, including excluded)

| Run | # | Task | Arm | Rep | Complete | Tools called | Requests | Finish reasons | Decoded | Cached / evaluated prompt | Wall (s) | Notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| pilot-1790430708089 | 0 | lookup | native | 1 | true | ["read_file"] | 2 | ["tool_calls","stop"] | 32.0 | 3480.0 / 3548.0 | 54.7 |  |
| pilot-1790430708089 | 1 | lookup | valve | 1 | true | ["read_file"] | 2 | ["tool_calls","stop"] | 78.0 | 2561.0 / 2701.0 | 57.6 |  |
| pilot-1790430708089 | 2 | edit | native | 1 | true | ["read_file","patch","write_file","patch","patch","patch"] | 7 | ["tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","stop"] | 558.0 | 23596.0 / 4575.0 | 256.8 | EXCLUDED |
| pilot-1790430708089 | 3 | edit | valve | 1 | true | ["patch","read_file","write_file","patch","write_file","write_file","read_file","write_file"] | 9 | ["tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","stop"] | 1519.0 | 28901.0 / 5337.0 | 572.6 | EXCLUDED |
| pilot-1790430708089 | 4 | create | native | 1 | true | ["search_files","read_file","read_file","write_file","write_file","write_file","write_file"] | 5 | ["tool_calls","tool_calls","tool_calls","tool_calls","stop"] | 459.0 | 15312.0 / 4638.0 | 202.4 | EXCLUDED |
| pilot-1790430708089 | 5 | create | valve | 1 | false | ["search_files","read_file","read_file","write_file","write_file","write_file","patch","patch","patch","patch","patch","patch"] | 13 | ["tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","stop"] | 1225.0 | 41858.0 / 8115.0 | 509.7 | EXCLUDED |
| pilot-1790430708089 | 6 | no_tool | native | 1 | true | [] | 1 | ["stop"] | 3.0 | 0.0 / 3455.0 | 27.1 |  |
| pilot-1790430708089 | 7 | no_tool | valve | 1 | true | [] | 1 | ["stop"] | 50.0 | 0.0 / 2566.0 | 38.7 |  |
| pilot-1790430708089 | 8 | lookup | valve | 2 | true | ["read_file"] | 2 | ["tool_calls","stop"] | 77.0 | 2561.0 / 2704.0 | 63.1 |  |
| pilot-1790430708089 | 9 | lookup | native | 2 | true | ["read_file"] | 2 | ["tool_calls","stop"] | 32.0 | 3480.0 / 3548.0 | 52.6 |  |
| pilot-1790430708089 | 10 | edit | valve | 2 | false | ["read_file","patch","search_files","patch","write_file","read_file","read_file","read_file","read_file","read_file","read_file","read_file"] | 14 | ["tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","stop"] | 995.0 | 43897.0 / 5705.0 | 406.3 | EXCLUDED |
| pilot-1790430708089 | 11 | edit | native | 2 | false | ["read_file","patch","write_file","patch","read_file"] | 6 | ["tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","stop"] | 371.0 | 18934.0 / 4023.0 | 180.6 | EXCLUDED |
| pilot-1790430708089 | 12 | create | valve | 2 | false | ["search_files","search_files","read_file","read_file","write_file","write_file","patch","patch","patch","patch","patch","patch"] | 13 | ["tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","stop"] | 867.0 | 39324.0 / 7198.0 | 381.9 | EXCLUDED |
| pilot-1790430708089 | 13 | create | native | 2 | true | ["search_files","read_file","read_file","write_file","write_file","write_file","write_file"] | 6 | ["tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","stop"] | 455.0 | 19968.0 / 4562.0 | 209.8 | EXCLUDED |
| pilot-1790430708089 | 14 | no_tool | valve | 2 | true | [] | 1 | ["stop"] | 61.0 | 0.0 / 2566.0 | 45.0 |  |
| pilot-1790430708089 | 15 | no_tool | native | 2 | true | [] | 1 | ["stop"] | 3.0 | 0.0 / 3455.0 | 33.1 |  |
| pilot-1790430708089 | 16 | lookup | native | 3 | true | ["read_file"] | 2 | ["tool_calls","stop"] | 32.0 | 3480.0 / 3548.0 | 39.8 |  |
| pilot-1790430708089 | 17 | lookup | valve | 3 | true | ["read_file"] | 2 | ["tool_calls","stop"] | 76.0 | 2561.0 / 2702.0 | 60.0 |  |
| pilot-1790430708089 | 18 | edit | native | 3 | true | ["read_file","patch","patch","write_file","write_file","write_file","read_file","write_file"] | 9 | ["tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","stop"] | 749.0 | 34230.0 / 5120.0 | 338.5 | EXCLUDED |
| pilot-1790430708089 | 19 | edit | valve | 3 | false | ["read_file","patch","patch","patch","patch","patch","patch","patch","write_file","patch","patch","patch"] | 14 | ["tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","length","tool_calls","stop"] | 2628.0 | 51515.0 / 9040.0 | 1008.7 | EXCLUDED |
| pilot-1790430708089 | 20 | create | native | 3 | true | ["search_files","read_file","read_file","write_file","write_file","write_file","write_file"] | 5 | ["tool_calls","tool_calls","tool_calls","tool_calls","stop"] | 479.0 | 15332.0 / 4638.0 | 209.4 | EXCLUDED |
| pilot-1790430708089 | 21 | create | valve | 3 | false | ["search_files","search_files","search_files","search_files","search_files","search_files","search_files","search_files","read_file","read_file","write_file","write_file"] | 13 | ["tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","stop"] | 800.0 | 37275.0 / 5725.0 | 401.1 | EXCLUDED |
| pilot-1790430708089 | 22 | no_tool | native | 3 | true | [] | 1 | ["stop"] | 3.0 | 0.0 / 3455.0 | 25.9 |  |
| pilot-1790430708089 | 23 | no_tool | valve | 3 | true | [] | 1 | ["stop"] | 41.0 | 0.0 / 2566.0 | 36.2 |  |
| pilot-1790436617596 | 0 | edit | native | 1 | true | ["read_file","patch"] | 3 | ["tool_calls","tool_calls","stop"] | 91.0 | 7042.0 / 4104.0 | 55.6 |  |
| pilot-1790436617596 | 1 | edit | valve | 1 | true | ["read_file","patch"] | 3 | ["tool_calls","tool_calls","stop"] | 122.0 | 5243.0 / 3249.0 | 57.9 |  |
| pilot-1790436617596 | 2 | create | native | 1 | true | ["search_files","read_file","read_file","write_file","write_file"] | 4 | ["tool_calls","tool_calls","tool_calls","stop"] | 193.0 | 10846.0 / 4325.0 | 82.6 |  |
| pilot-1790436617596 | 3 | create | valve | 1 | true | ["search_files","search_files","read_file","read_file","write_file","write_file"] | 7 | ["tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","stop"] | 279.0 | 17269.0 / 3631.0 | 114.0 |  |
| pilot-1790436617596 | 4 | edit | valve | 2 | true | ["search_files","read_file","patch"] | 4 | ["tool_calls","tool_calls","tool_calls","stop"] | 158.0 | 7984.0 / 3297.0 | 73.4 |  |
| pilot-1790436617596 | 5 | edit | native | 2 | true | ["read_file","patch"] | 3 | ["tool_calls","tool_calls","stop"] | 91.0 | 7042.0 / 4104.0 | 49.6 |  |
| pilot-1790436617596 | 6 | create | valve | 2 | true | ["search_files","read_file","read_file","write_file","write_file"] | 6 | ["tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","stop"] | 240.0 | 14135.0 / 3520.0 | 99.3 |  |
| pilot-1790436617596 | 7 | create | native | 2 | true | ["search_files","read_file","read_file","write_file","write_file"] | 4 | ["tool_calls","tool_calls","tool_calls","stop"] | 193.0 | 10935.0 / 4236.0 | 81.4 |  |
| pilot-1790436617596 | 8 | edit | native | 3 | true | ["read_file","patch"] | 3 | ["tool_calls","tool_calls","stop"] | 91.0 | 7042.0 / 4104.0 | 53.7 |  |
| pilot-1790436617596 | 9 | edit | valve | 3 | true | ["read_file","patch"] | 3 | ["tool_calls","tool_calls","stop"] | 157.0 | 5243.0 / 3262.0 | 66.1 |  |
| pilot-1790436617596 | 10 | create | native | 3 | true | ["search_files","read_file","read_file","write_file","write_file"] | 4 | ["tool_calls","tool_calls","tool_calls","stop"] | 193.0 | 10846.0 / 4325.0 | 80.6 |  |
| pilot-1790436617596 | 11 | create | valve | 3 | true | ["search_files","search_files","read_file","read_file","write_file","write_file"] | 7 | ["tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","tool_calls","stop"] | 311.0 | 17360.0 / 3553.0 | 125.1 |  |

## Limits and open measurements

- **Tool catalog size:** 4 tools offered (Hermes's `file` toolset). The Hermes-sized catalog measurement (INT-0013 AC-2) remains open.
- **Arms:** only native (template tool grammar, lazy) versus today's `F-thought` schema. `N-forced`, `F-bounded`, `F-none` and the reasoning axis (INT-0013 AC-4) are not measured here. Thinking was off in both arms.
- **Sample:** three repetitions per task and arm. Differences smaller than session-to-session variation are not evidence either way.
- **History rendering:** the valve re-renders prior actions canonically. The cached and evaluated prompt columns show what that costs per session.
- **Cleanup:** every child was reaped: **true** (38 cleanup records).

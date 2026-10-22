# Sprint 126 Meta

- **Sprint number:** 126
- **Book schema version:** 2
- **Start timestamp:** 2026-09-26T07:02:20Z
- **End timestamp:** 2026-09-27T04:41:00Z
- **Model:** claude-opus-5-5
- **Bundle version:** 0.22.0
- **Exit status:** success
- **Token count:** (filled at Loop Phase if observable)
- **Summary:** Reset intents around Ferric as the Iron of Animus Amalgam. Extract the constrained-decoding core (`ferric-iron`) and measure the extraction, then put the constrained valve between real Hermes and llama.cpp and run a native-vs-valve pilot on the 27B Q4.
- **Intents:** [INT-0011](../../intents/INT-0011-standalone-constrained-decoding-core.md), [INT-0012](../../intents/INT-0012-constrained-valve-at-hermes-boundary.md), [INT-0013](../../intents/INT-0013-constrained-decoding-on-midsize-quantized-target.md), [INT-0010](../../intents/INT-0010-ferric-is-the-iron-of-amalgam.md)
- **Completion evidence:** INT-0011/0012/0013 Iron extraction and valve: `ferric-iron` constrained-decoding core extracted and measured (T-12601..T-12605), the valve between Hermes and llama.cpp (T-12606, T-12607), real Hermes through the valve with process-tree reaping (T-12608), the 27B native-vs-valve pilot (T-12609: native 12/12, valve 12/12, every tool call valid in both arms), Iron charter docs (T-12610); follow-ups T-12611..T-12623 folded into INT-0011..0014 as backlog. CI green on `dev` at 508edfd, critique proceed-with-caveats. The unit, integration and summary test-report files were left empty; the e2e and extraction-measurement reports carry the evidence. Closed by owner direction on 2026-10-22, ahead of sprint 127.

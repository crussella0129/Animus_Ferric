# Agent Tasks (Persistent Backlog)

> Sprint 11: re-enable constrained decoding on the mistral.rs backend. `complete()`
> currently strips the constraint (ADR-020 workaround); the hang is fixed in 0.8.15
> and mistralrs exposes `RequestBuilder::set_constraint`. Wire it, then the bounded
> `grammar_probe` decides enforce/ignore/hang → ADR-027. Plan: `sprints/s11/sprint-plans/build-plan.md`.

- [ ] T-1101 (sprint 11): Pass `Constraint` to the mistralrs `RequestBuilder` (`to_mistralrs_constraint` + `set_constraint`; `supports_constraint` stays false provisionally) — touches: crates/ferric-provider/src/mistralrs.rs

Test/loop (post-build): run the bounded `grammar_probe` (trivial + unified) on the
1B GGUF → enforce/ignore/hang; ADR-027 records the verdict; flip
`supports_constraint:true` only if it enforces without hanging.

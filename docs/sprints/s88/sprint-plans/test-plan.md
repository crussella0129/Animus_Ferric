# Finalized - DO NOT EDIT

# Sprint 88 — Test Plan

## G1

| Case | Expected |
|---|---|
| single-word query | matches |
| **multi-word query** | matches (the regression) |
| terms from different files | no match — terms are ANDed |
| word order / punctuation | irrelevant |
| blank query | matches nothing (never sweep the tree into the quarantine) |
| filename term | still matches |

## Live

The exact query that silently returned nothing must now inject a
`<research_context>`. Then, with a real digest present, observe whether a
digest-derived `write_file` is allowed or denied — the E2 measurement that
sprints 85–87 could not make.

## Gate

`cargo test --workspace` > 518, clippy 0, fmt clean.

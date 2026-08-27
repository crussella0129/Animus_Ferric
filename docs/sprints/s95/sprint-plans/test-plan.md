# Finalized - DO NOT EDIT

# Sprint 95 — Test Plan

## Live calibration

Full L0–L6 ladder per model, release build with `--features backend-openai`
against `ferric server` + llama.cpp. Repeat any level whose result contradicts
its neighbours before believing it.

## Unit — the calibrator

| Case | Expected |
|---|---|
| fail L5, pass L6 | `measured_level` 6 **and** L5 reported as inconsistent |
| clean ladder | nothing reported |
| fail L5 **and** L6 after passing L4 | nothing reported — a plain ceiling, not a contradiction |
| nothing passed | no level, nothing reported |

Row 3 is the false-positive boundary: a ceiling is the normal shape of a result
and must not be dressed up as an anomaly.

## Live regression

`bench full --level N` must print `profile left unchanged` and leave
`model_profiles.json` untouched.

## Gate

`cargo test --workspace` > 545, clippy 0, fmt clean.

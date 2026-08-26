# Sprint 88 — Test Report

## Gate

| Check | Result |
|---|---|
| `cargo test --workspace` | **524 passed / 0 failed** (518 at sprint start, +6) |
| clippy / fmt | 0 warnings / clean |

## G1 — isolated, fixed, validated live

Probe result that isolated it:

```
SPRINT88_PROBE chunks=1            # query "configuration"
SPRINT88_PROBE multiword chunks=0  # query "project notes configuration"
```

Same directory, same file. The query was matched as one literal lowercase
substring. 6 permanent tests now cover single-word, multi-word, conjunctive
terms, order/punctuation independence, blank-query safety, and filename matches.

Live, the previously-silent query now injects a real digest:

```
research_context injected?: True
  "The project is a Rust workspace with multiple crates, and the configuration
   file is located at the repository root. Tests are executed using cargo test…"
```

## E2 — measured live, and it corrects ADR-075

ADR-075 reported **3/3 benign restatements blocked**. Live, with a real digest:

```
CALL write_file {"path":"summary2.txt","content":"The configuration file is located at the repository root."}
   -> ok: wrote 57 bytes
```

**Allowed, not blocked.** Two reasons the synthetic probe overstated it:

1. The digest is a model **paraphrase** — the source said "lives at", the digest
   said "is located at". Needles never equal the file's own words.
2. The needle is the whole sentence *"…multiple crates, and the configuration
   file is located at the repository root"*; the model wrote a **prefix** of it.
   `is_tainted` asks whether the *argument contains the needle*, so a shorter
   restatement cannot match.

**The honest reading:** the mechanism is weak in *both* directions. The
false-positive rate is lower than claimed — and so is the protection, since a
paraphrased or partially-quoted injection evades it exactly the same way.

## Method note

The synthetic probe was right to exist — it isolated the mechanism. It was wrong
to be trusted as a **rate**. A probe that constructs its own worst case measures
the worst case, not the field. Both numbers are kept in the record.

## Not run

A5's sandbox (Docker absent) and the fleet re-calibration (the 3B is now local
for it).

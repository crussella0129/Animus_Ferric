# Plan Critique — Sprint 10

> Self-critique against `prompts/plan-critic.md`.

## Concerns

### C-001: The live-media E2E can't run on this machine
- **Failure mode:** untestable-acceptance
- **Response:** **accept + defer-with-rationale.** This is the known wall (ADR-025): no multimodal server present. Mitigation is structural — the entire pipeline is pure (`classify_path`, `decide_attachment`, `map_message`, the parts array) and unit-tested; only "a real model reads a PNG" is deferred, and it's recorded as an explicit checkpoint in the test-report, not a silent gap. The sprint still ships verifiable value (a working, tested input pipeline).

### C-002: `Message` change ripples across core/provider/loop/trace
- **Failure mode:** blast-radius
- **Response:** **mitigated by design.** The field is **additive** (`media: Vec<MediaPart>`, `#[serde(default, skip_serializing_if = "Vec::is_empty")]`) — `text` stays, every `msg.text` reader is untouched, and a media-free message serializes byte-identically (a unit-tested invariant). Construction sites that use struct literals (`Message { .. }`) need the new field; the `Message::user`/`system`/etc. constructors absorb it so call sites mostly don't change.

### C-003: modality as a CLI flag vs `ModelProfile` field (ADR-006)
- **Failure mode:** ignored-ADR (boundary)
- **Response:** **reject — still ADR-006-clean.** `--modality` is *explicit config*, never inferred; ADR-006 forbids sniffing, not CLI declaration. Keeping it on `QueryArgs` (not `ModelProfile`) avoids touching the snapshot-tested `ModelProfile` and its construction sites for a query-only concern. A future `ModelProfile.modalities` (for trace/profile records) is a clean follow-on, not needed for input.

### C-004: `Capabilities` gains a field → every construction site
- **Failure mode:** compile-breakage
- **Response:** **accept (small, bounded).** Mock + openai + mistral `Capabilities { .. }` sites get `supports_media`. Caught immediately by the compiler; trivial.

## Confidence
`proceed-with-caveats` — additive, mostly-pure feature; the only non-AI-verifiable piece (live media) is explicitly deferred with the pipeline fully unit-tested underneath it.

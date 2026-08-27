Finalized - DO NOT EDIT

# Sprint 10 Build Plan — Multimodal "any file" input

`ferric query` takes any file: text/code as text (any model), image/audio/video
as capability-gated media parts on the OpenAI valve (ADR-023/025). The whole
pipeline is pure + unit-tested this sprint; the live-media E2E is deferred to a
heartbeat (no multimodal server on the machine). Rationale:
`sprints/s10/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** any file → message content, capability-gated, on the OpenAI valve.
  - **A. Core data model + routing logic** — T-1001
  - **B. Provider mapping + honest capability** — T-1002
  - **C. CLI `--file` wiring** — T-1003
  - **D. Docs** — T-1004

## Execution Sequence

### T-1001: `Message` media parts + routing logic (pure)
- **Touches:** `crates/ferric-core/src/message.rs`, `crates/ferric-core/src/lib.rs`
- **Depends on:** (none)
- **Success (EARS):**
  - `Message` **SHALL** carry additive `media: Vec<MediaPart{mime, data(base64)}>` (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`) — media-free messages serialize byte-identically to today.
  - `classify_path(path) -> FileKind` (`Text | Media(Modality,mime) | Unknown`) **SHALL** map by extension.
  - `decide_attachment(kind, declared, backend_supports_media) -> Attachment` (`AppendText | Media | Skip(reason)`) **SHALL** attach media only when declared ∧ backend-carries-media, else `Skip(reason)`.
- **Notes:** `Modality{Image,Audio,Video}`, `MediaPart`, `Message::user_with_media`. Unit-test roundtrip + `classify_path` + every `decide_attachment` branch.

### T-1002: OpenAI multimodal content mapping + honest capability
- **Touches:** `crates/ferric-provider/src/openai.rs`, `crates/ferric-provider/src/types.rs`, `crates/ferric-provider/src/mistralrs.rs` (capabilities, if present)
- **Depends on:** T-1001
- **Success (EARS):**
  - WHEN `media` non-empty, **THEN** `map_message` content **SHALL** be the OpenAI parts array (`text` + `image_url` data-URL / `input_audio`); WHEN empty, a plain string (unchanged).
  - `Capabilities` **SHALL** gain `supports_media` — `true` (valve), `false` (mistral.rs) — honest per ADR-022.
- **Notes:** pure `build_body`/`map_message` unit-tested both branches; add `supports_media` to every `Capabilities { .. }` site.

### T-1003: `ferric query --file` input + gating wiring
- **Touches:** `crates/ferric-cli/src/query.rs`
- **Depends on:** T-1001, T-1002
- **Success (EARS):**
  - `ferric query` **SHALL** accept repeatable `--file <path>` + `--modality <image,audio,video>` (explicit config, ADR-006).
  - Each `--file` **SHALL** route via `classify_path`+`decide_attachment`: Text → append to prompt (any model); Media → gated `MediaPart`; Skip → clear stderr note, never silent.
- **Notes:** logic is T-1001's pure fns; this is file-read + Message assembly + a `--mock` integration test.

### T-1004: Docs + timeline
- **Touches:** `README.md`, `docs/multimodal.md` (new)
- **Depends on:** T-1001–T-1003
- **Success (EARS):** docs **SHALL** show `ferric query --file … [--modality …]`, state text=any-model / media=multimodal-model+server, and append the Sprint 10 README timeline line.

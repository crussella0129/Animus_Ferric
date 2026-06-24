//! Multimodal input: file → message-content routing (ADR-023).
//!
//! Turning "any file" into message content has two *pure* decisions, kept here
//! so they're testable without I/O or a backend: (1) what kind of file is this
//! (`classify_path`, by extension), and (2) given the model's declared
//! modalities and whether the backend can carry media at all, do we attach it
//! as a media part, fold it in as text, or skip it (`decide_attachment`). The
//! actual file read + base64 + `Message` assembly lives in the CLI; this is the
//! logic.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// A non-text input modality a model may accept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    Image,
    Audio,
    Video,
}

/// One non-text content part on a `Message`: a media payload, base64-encoded,
/// tagged with its MIME type. Bytes are base64 so a `Message` stays plain
/// serde-JSON; traces must log a descriptor (mime + length), never this blob.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaPart {
    pub mime: String,
    /// Base64-encoded payload (no `data:` prefix).
    pub data: String,
}

/// What `classify_path` decided a file is, by extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileKind {
    /// Text/code — folds into the prompt as text; works on any model.
    Text,
    /// A media file: its modality + MIME type.
    Media(Modality, String),
    /// Unrecognized extension.
    Unknown,
}

/// How a file should be attached to the request, after gating.
#[derive(Debug, Clone, PartialEq)]
pub enum Attachment {
    /// Read it and append to the prompt text.
    AppendText,
    /// Attach as a media part (modality + MIME).
    Media(Modality, String),
    /// Don't attach; the reason is human-readable (surfaced, never silent).
    Skip(String),
}

/// Classify a path by extension. Pure; no filesystem access.
pub fn classify_path(path: &Path) -> FileKind {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        // Text / code — the universal "any file" baseline.
        "rs" | "md" | "txt" | "json" | "toml" | "yaml" | "yml" | "py" | "go" | "js" | "ts"
        | "c" | "h" | "cpp" | "hpp" | "java" | "sh" | "ps1" | "html" | "css" | "xml" | "csv"
        | "log" | "cfg" | "ini" | "" => FileKind::Text,
        // Image.
        "png" => FileKind::Media(Modality::Image, "image/png".to_string()),
        "jpg" | "jpeg" => FileKind::Media(Modality::Image, "image/jpeg".to_string()),
        "webp" => FileKind::Media(Modality::Image, "image/webp".to_string()),
        "gif" => FileKind::Media(Modality::Image, "image/gif".to_string()),
        // Audio.
        "wav" => FileKind::Media(Modality::Audio, "audio/wav".to_string()),
        "mp3" => FileKind::Media(Modality::Audio, "audio/mpeg".to_string()),
        "flac" => FileKind::Media(Modality::Audio, "audio/flac".to_string()),
        "ogg" => FileKind::Media(Modality::Audio, "audio/ogg".to_string()),
        // Video.
        "mp4" => FileKind::Media(Modality::Video, "video/mp4".to_string()),
        "mov" => FileKind::Media(Modality::Video, "video/quicktime".to_string()),
        "webm" => FileKind::Media(Modality::Video, "video/webm".to_string()),
        _ => FileKind::Unknown,
    }
}

/// Decide how to attach a classified file, given the model's *declared*
/// modalities (explicit config, ADR-006 — never inferred) and whether the
/// backend can carry media at all. Media attaches only when its modality is
/// declared AND the backend carries media; otherwise it is skipped with a
/// human-readable reason. Unknown files are skipped (not blindly sent as bytes).
pub fn decide_attachment(
    kind: &FileKind,
    declared: &[Modality],
    backend_supports_media: bool,
) -> Attachment {
    match kind {
        FileKind::Text => Attachment::AppendText,
        FileKind::Unknown => Attachment::Skip(
            "unrecognized file type (not text or a known media format)".to_string(),
        ),
        FileKind::Media(m, mime) => {
            if !backend_supports_media {
                Attachment::Skip(format!(
                    "backend cannot carry media; {mime} not sent (use the OpenAI valve)"
                ))
            } else if !declared.contains(m) {
                Attachment::Skip(format!(
                    "model not declared to accept {} input; pass --modality {} to enable",
                    modality_flag(*m),
                    modality_flag(*m)
                ))
            } else {
                Attachment::Media(*m, mime.clone())
            }
        }
    }
}

/// The `--modality` flag spelling of a `Modality`.
pub fn modality_flag(m: Modality) -> &'static str {
    match m {
        Modality::Image => "image",
        Modality::Audio => "audio",
        Modality::Video => "video",
    }
}

/// Parse a `--modality` value (`image,audio,video`) into modalities; unknown
/// tokens are ignored. Pure; used by the CLI.
pub fn parse_modalities(s: &str) -> Vec<Modality> {
    s.split(',')
        .filter_map(|t| match t.trim().to_ascii_lowercase().as_str() {
            "image" => Some(Modality::Image),
            "audio" => Some(Modality::Audio),
            "video" => Some(Modality::Video),
            _ => None,
        })
        .collect()
}

/// Standard base64 encoding (RFC 4648, no line breaks). Inlined and
/// dependency-free (ADR-004): just enough to fold media bytes into a `data:`
/// URL / `input_audio` payload, without pulling a crate for ~15 lines.
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_by_extension() {
        assert_eq!(classify_path(Path::new("src/main.rs")), FileKind::Text);
        assert_eq!(classify_path(Path::new("README.md")), FileKind::Text);
        assert_eq!(classify_path(Path::new("data.json")), FileKind::Text);
        assert!(matches!(
            classify_path(Path::new("a.png")),
            FileKind::Media(Modality::Image, _)
        ));
        // Case-insensitive.
        assert!(matches!(
            classify_path(Path::new("PHOTO.JPG")),
            FileKind::Media(Modality::Image, _)
        ));
        assert!(matches!(
            classify_path(Path::new("clip.wav")),
            FileKind::Media(Modality::Audio, _)
        ));
        assert!(matches!(
            classify_path(Path::new("movie.mp4")),
            FileKind::Media(Modality::Video, _)
        ));
        assert_eq!(classify_path(Path::new("mystery.xyz")), FileKind::Unknown);
    }

    #[test]
    fn text_always_appends() {
        assert_eq!(
            decide_attachment(&FileKind::Text, &[], false),
            Attachment::AppendText
        );
    }

    #[test]
    fn media_needs_declared_and_supported() {
        let img = FileKind::Media(Modality::Image, "image/png".to_string());
        // declared + backend carries media → attach
        assert!(matches!(
            decide_attachment(&img, &[Modality::Image], true),
            Attachment::Media(Modality::Image, _)
        ));
        // undeclared modality → skip with a reason
        assert!(matches!(
            decide_attachment(&img, &[], true),
            Attachment::Skip(_)
        ));
        // declared, but a text-only backend (mistral.rs) → skip with a reason
        assert!(matches!(
            decide_attachment(&img, &[Modality::Image], false),
            Attachment::Skip(_)
        ));
    }

    #[test]
    fn unknown_skips() {
        assert!(matches!(
            decide_attachment(&FileKind::Unknown, &[Modality::Image], true),
            Attachment::Skip(_)
        ));
    }

    #[test]
    fn parse_modalities_tolerant() {
        assert_eq!(
            parse_modalities("image, audio"),
            vec![Modality::Image, Modality::Audio]
        );
        assert_eq!(parse_modalities("video"), vec![Modality::Video]);
        assert_eq!(parse_modalities("bogus"), Vec::<Modality>::new());
    }

    #[test]
    fn base64_rfc4648_vectors() {
        // The canonical RFC 4648 §10 test vectors, incl. both padding cases.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }
}

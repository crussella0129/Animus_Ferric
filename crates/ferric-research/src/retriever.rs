//! Ornstein research sources: the `Retriever` keystone + the Local-FS plane.
//!
//! Ornstein is "one funnel, many sources." Every [`Retriever`] returns raw,
//! UNTRUSTED chunks with provenance; [`research`] runs them all through the
//! quarantine ([`summarize_quarantined`](crate::summarize_quarantined)) into
//! typed [`ResearchDigest`]s. The funnel is source-agnostic, so each plane
//! (local FS now; tailnet/NAS + web next) is an additive `Retriever`, not a
//! rewrite.

use std::fs::DirEntry;
use std::path::{Path, PathBuf};
use std::process::Command;

use async_trait::async_trait;
use ferric_provider::Provider;
use thiserror::Error;

use crate::{ResearchDigest, ResearchError, summarize_quarantined};

/// Directories never walked for research content (build / VCS noise).
const NOISE_DIRS: &[&str] = &[".git", "target", "node_modules", ".ferric"];

/// A raw, UNTRUSTED chunk retrieved from a source, carrying its provenance.
/// Goes straight to the quarantine — never to the planner.
#[derive(Debug, Clone, PartialEq)]
pub struct RetrievedChunk {
    /// Provenance: where it came from (a file relpath, later a URL or a
    /// tailnet `device:path`).
    pub source: String,
    /// The untrusted raw content.
    pub content: String,
}

/// A retrieval failure.
#[derive(Debug, Error)]
pub enum RetrieveError {
    #[error("retrieve io: {0}")]
    Io(String),
    #[error("retrieve exec: {0}")]
    Exec(String),
}

/// A research **source plane**. `available()` is a runtime capability probe (a
/// network/tailnet plane may be offline); `plane()` labels it. Every plane —
/// local FS, tailnet/NAS, web — implements this one trait and feeds the same
/// quarantine.
#[async_trait]
pub trait Retriever: Send + Sync {
    /// A short label for the source plane: `"local"` | `"tailnet"` | `"web"`.
    fn plane(&self) -> &str;
    /// Whether this source can run right now (capability probe).
    fn available(&self) -> bool;
    /// Retrieve candidate UNTRUSTED chunks relevant to `query`.
    async fn retrieve(&self, query: &str) -> Result<Vec<RetrievedChunk>, RetrieveError>;
}

/// Run a research query against a retriever, **quarantining every chunk**. The
/// `provider` is the quarantined summarizer model. An *unavailable* retriever is
/// a no-op (empty result), not an error — a capability-probed multi-source
/// system runs only the planes that are live.
pub async fn research(
    retriever: &dyn Retriever,
    provider: &dyn Provider,
    query: &str,
) -> Result<Vec<ResearchDigest>, ResearchError> {
    if !retriever.available() {
        return Ok(Vec::new());
    }
    let chunks = retriever
        .retrieve(query)
        .await
        .map_err(|e| ResearchError::Retrieve(e.to_string()))?;
    let mut digests = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        digests.push(summarize_quarantined(provider, &chunk.source, &chunk.content, query).await?);
    }
    Ok(digests)
}

/// The Local-FS source plane: search files under a confined `root` for a query,
/// returning matching files as untrusted chunks. Confined to `root`, symlinks
/// are not followed (escape-safety); the *content* is still untrusted (a local
/// file can carry an injection), so it goes through the quarantine like any
/// other source.
pub struct LocalFsRetriever {
    root: PathBuf,
    max_files: usize,
    max_bytes_per_file: usize,
}

impl LocalFsRetriever {
    /// Default caps: 20 files, 64 KiB each.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self::with_caps(root, 20, 64 * 1024)
    }

    pub fn with_caps(
        root: impl Into<PathBuf>,
        max_files: usize,
        max_bytes_per_file: usize,
    ) -> Self {
        Self {
            root: root.into(),
            max_files,
            max_bytes_per_file,
        }
    }
}

#[async_trait]
impl Retriever for LocalFsRetriever {
    fn plane(&self) -> &str {
        "local"
    }

    fn available(&self) -> bool {
        self.root.is_dir()
    }

    async fn retrieve(&self, query: &str) -> Result<Vec<RetrievedChunk>, RetrieveError> {
        let needle = query.to_lowercase();
        let mut out = Vec::new();
        walk(
            &self.root,
            &self.root,
            &needle,
            self.max_files,
            self.max_bytes_per_file,
            &mut out,
        )?;
        Ok(out)
    }
}

/// Recurse `dir` (sorted, ADR-008), collecting files whose name or content
/// contains `needle` (already lowercased). Skips noise dirs, symlinks, and
/// binary/unreadable files. Caps the result count and each file's bytes.
fn walk(
    dir: &Path,
    root: &Path,
    needle: &str,
    max_files: usize,
    max_bytes: usize,
    out: &mut Vec<RetrievedChunk>,
) -> Result<(), RetrieveError> {
    if out.len() >= max_files {
        return Ok(());
    }
    let mut entries: Vec<DirEntry> = std::fs::read_dir(dir)
        .map_err(|e| RetrieveError::Io(format!("{}: {e}", dir.display())))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(DirEntry::file_name);

    for entry in entries {
        if out.len() >= max_files {
            break;
        }
        // file_type() does NOT follow symlinks — so we can detect + skip them.
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();

        if ft.is_dir() {
            if NOISE_DIRS.contains(&name.as_str()) {
                continue;
            }
            walk(&path, root, needle, max_files, max_bytes, out)?;
        } else {
            // Binary / unreadable files fall away here (non-UTF-8 → Err).
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let name_match = name.to_lowercase().contains(needle);
            let content_match = content.to_lowercase().contains(needle);
            if name_match || content_match {
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push(RetrievedChunk {
                    source: rel,
                    content: cap_bytes(content, max_bytes),
                });
            }
        }
    }
    Ok(())
}

/// Truncate `s` to at most `max` bytes, on a char boundary.
fn cap_bytes(s: String, max: usize) -> String {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

// ---------------------------------------------------------------------------
// Tailnet/NAS-FS plane (Ornstein increment 3): search a REMOTE device's
// filesystem over SSH and feed matches to the same quarantine.
//
// `ssh` runs its command through the remote shell, so the caller-supplied query
// and remote root MUST be POSIX single-quote-escaped or it is remote command
// injection. The escaping + argv builders are pure (unit-tested); only the
// process spawn is live (deferred E2E).
// ---------------------------------------------------------------------------

/// How to reach a tailnet host's shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshTransport {
    /// `tailscale ssh <host> -- <cmd>` — keyless, identity-based; Linux tailnet devices.
    Tailscale,
    /// `ssh -p <port> -o BatchMode=yes -o ConnectTimeout=8 <host> -- <cmd>` — e.g. Termux sshd.
    Plain { port: u16 },
}

/// A device seen in `tailscale status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailnetDevice {
    pub name: String,
    pub ip: String,
    pub online: bool,
}

/// POSIX single-quote `s` so a remote shell treats it as one literal argument.
/// Wraps in `'…'`; an embedded `'` becomes the `'\''` splice. This is the
/// defense against remote command injection via the research query.
pub fn shell_single_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Wrap a remote shell command in the transport's argv → `(program, args)`.
fn ssh_argv(transport: &SshTransport, host: &str, remote_cmd: &str) -> (String, Vec<String>) {
    match transport {
        SshTransport::Tailscale => (
            "tailscale".to_string(),
            vec![
                "ssh".to_string(),
                host.to_string(),
                "--".to_string(),
                remote_cmd.to_string(),
            ],
        ),
        SshTransport::Plain { port } => (
            "ssh".to_string(),
            vec![
                "-p".to_string(),
                port.to_string(),
                "-o".to_string(),
                "BatchMode=yes".to_string(),
                "-o".to_string(),
                "ConnectTimeout=8".to_string(),
                host.to_string(),
                "--".to_string(),
                remote_cmd.to_string(),
            ],
        ),
    }
}

/// Argv to list (capped) files under `remote_root` on `host` matching `query`.
/// The query + root are single-quoted into the remote command (injection-safe).
pub fn ssh_search_argv(
    transport: &SshTransport,
    host: &str,
    query: &str,
    remote_root: &str,
    max_files: usize,
) -> (String, Vec<String>) {
    let cmd = format!(
        "grep -rIl -- {} {} | head -n {}",
        shell_single_quote(query),
        shell_single_quote(remote_root),
        max_files
    );
    ssh_argv(transport, host, &cmd)
}

/// Argv to read one remote file (path single-quoted).
pub fn ssh_cat_argv(
    transport: &SshTransport,
    host: &str,
    remote_path: &str,
) -> (String, Vec<String>) {
    let cmd = format!("cat -- {}", shell_single_quote(remote_path));
    ssh_argv(transport, host, &cmd)
}

/// Parse `tailscale status` stdout into devices. Each device line is
/// `IP  name  owner  os  [status…]`; a line containing "offline" ⇒ `online=false`.
/// Non-device lines (no `100.x` CGNAT IP first) are ignored.
pub fn parse_status_devices(stdout: &str) -> Vec<TailnetDevice> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut cols = line.split_whitespace();
            let ip = cols.next()?;
            let name = cols.next()?;
            if !ip.starts_with("100.") {
                return None; // not a tailnet (CGNAT 100.64/10) device line
            }
            let online = !line.to_lowercase().contains("offline");
            Some(TailnetDevice {
                name: name.to_string(),
                ip: ip.to_string(),
                online,
            })
        })
        .collect()
}

/// The Tailnet/NAS-FS source plane: search a remote tailnet device's filesystem
/// over SSH (Tailscale SSH for Linux devices; plain `ssh -p` for Termux-style
/// sshd). All retrieved content is untrusted → the quarantine, like any source.
pub struct TailnetFsRetriever {
    host: String,
    remote_root: String,
    transport: SshTransport,
    max_files: usize,
    max_bytes_per_file: usize,
}

impl TailnetFsRetriever {
    /// Default caps: 20 files, 64 KiB each.
    pub fn new(
        host: impl Into<String>,
        remote_root: impl Into<String>,
        transport: SshTransport,
    ) -> Self {
        Self {
            host: host.into(),
            remote_root: remote_root.into(),
            transport,
            max_files: 20,
            max_bytes_per_file: 64 * 1024,
        }
    }

    /// Live: `tailscale status` → parsed devices (empty on any failure).
    fn status_devices() -> Vec<TailnetDevice> {
        match Command::new("tailscale").arg("status").output() {
            Ok(o) if o.status.success() => {
                parse_status_devices(&String::from_utf8_lossy(&o.stdout))
            }
            _ => Vec::new(),
        }
    }
}

#[async_trait]
impl Retriever for TailnetFsRetriever {
    fn plane(&self) -> &str {
        "tailnet"
    }

    fn available(&self) -> bool {
        Self::status_devices()
            .iter()
            .any(|d| d.name == self.host && d.online)
    }

    async fn retrieve(&self, query: &str) -> Result<Vec<RetrievedChunk>, RetrieveError> {
        // 1. Remote search → matching file paths.
        let (prog, args) = ssh_search_argv(
            &self.transport,
            &self.host,
            query,
            &self.remote_root,
            self.max_files,
        );
        let search = Command::new(&prog)
            .args(&args)
            .output()
            .map_err(|e| RetrieveError::Exec(format!("{prog}: {e}")))?;
        if !search.status.success() {
            return Err(RetrieveError::Exec(format!(
                "remote search failed: {}",
                String::from_utf8_lossy(&search.stderr).trim()
            )));
        }
        let files: Vec<String> = String::from_utf8_lossy(&search.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .take(self.max_files)
            .map(String::from)
            .collect();

        // 2. Read each match → an untrusted chunk tagged `host:relpath`.
        let mut chunks = Vec::with_capacity(files.len());
        for file in files {
            let (cprog, cargs) = ssh_cat_argv(&self.transport, &self.host, &file);
            let cat = Command::new(&cprog)
                .args(&cargs)
                .output()
                .map_err(|e| RetrieveError::Exec(format!("{cprog}: {e}")))?;
            if !cat.status.success() {
                continue; // skip a file we couldn't read
            }
            let content = cap_bytes(
                String::from_utf8_lossy(&cat.stdout).into_owned(),
                self.max_bytes_per_file,
            );
            chunks.push(RetrievedChunk {
                source: format!("{}:{}", self.host, file),
                content,
            });
        }
        Ok(chunks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_core::Message;
    use ferric_provider::{Completion, MockProvider};

    fn block<F: std::future::Future>(f: F) -> F::Output {
        futures_executor::block_on(f)
    }

    fn write(dir: &Path, rel: &str, content: &[u8]) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    #[test]
    fn matches_by_content_and_excludes_non_matches() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "notes.md", b"all about tailscale and NAT");
        write(dir.path(), "other.txt", b"nothing relevant here");
        let r = LocalFsRetriever::new(dir.path());
        let chunks = block(r.retrieve("tailscale")).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].source, "notes.md");
        assert!(chunks[0].content.contains("tailscale"));
    }

    #[test]
    fn matches_by_name_and_is_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Tailscale-setup.txt", b"body has no keyword");
        let r = LocalFsRetriever::new(dir.path());
        let chunks = block(r.retrieve("TAILSCALE")).unwrap();
        assert_eq!(chunks.len(), 1, "name match, case-insensitive");
        assert_eq!(chunks[0].source, "Tailscale-setup.txt");
    }

    #[test]
    fn skips_noise_dirs_and_binary_files() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".git/config", b"tailscale in a noise dir");
        write(dir.path(), "bin.dat", &[0xff, 0xfe, 0x00, 0x01]); // non-UTF-8
        write(dir.path(), "good.txt", b"tailscale here");
        let r = LocalFsRetriever::new(dir.path());
        let chunks = block(r.retrieve("tailscale")).unwrap();
        let sources: Vec<&str> = chunks.iter().map(|c| c.source.as_str()).collect();
        assert_eq!(sources, vec!["good.txt"], "noise dir + binary skipped");
    }

    #[test]
    fn respects_max_files_cap() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..5 {
            write(dir.path(), &format!("f{i}.txt"), b"tailscale");
        }
        let r = LocalFsRetriever::with_caps(dir.path(), 2, 64 * 1024);
        let chunks = block(r.retrieve("tailscale")).unwrap();
        assert_eq!(chunks.len(), 2, "capped to max_files");
    }

    #[test]
    fn availability_and_plane() {
        let dir = tempfile::tempdir().unwrap();
        let ok = LocalFsRetriever::new(dir.path());
        assert!(ok.available());
        assert_eq!(ok.plane(), "local");
        let missing = LocalFsRetriever::new(dir.path().join("does-not-exist"));
        assert!(!missing.available());
    }

    fn digest_completion(json_text: &str) -> Completion {
        Completion {
            message: Message::assistant(json_text),
            input_tokens: Some(20),
            output_tokens: Some(40),
            truncated: false,
        }
    }

    #[test]
    fn research_pipeline_source_to_quarantined_digest() {
        // The headline: a real file on disk → a quarantined, provenance-tagged
        // digest. One matching file ⇒ one quarantine call ⇒ one digest.
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "paper.md", b"tailscale enables NAT traversal");
        let r = LocalFsRetriever::new(dir.path());
        let mock = MockProvider::new(vec![digest_completion(
            r#"{"summary":"about tailscale NAT","claims":[{"claim":"NAT traversal","quote":"tailscale enables NAT traversal"}]}"#,
        )]);
        let digests = block(research(&r, &mock, "tailscale")).unwrap();
        assert_eq!(digests.len(), 1);
        assert!(
            digests[0].untrusted,
            "provenance: harness-stamped untrusted"
        );
        assert_eq!(digests[0].source, "paper.md", "provenance: the source file");
    }

    #[test]
    fn research_on_unavailable_retriever_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let r = LocalFsRetriever::new(dir.path().join("nope"));
        let mock = MockProvider::new(vec![]);
        let digests = block(research(&r, &mock, "tailscale")).unwrap();
        assert!(digests.is_empty(), "unavailable plane → no-op, not error");
    }

    // --- Tailnet plane (pure core; the live spawn is the deferred E2E) ---

    #[test]
    fn shell_single_quote_neutralizes_injections() {
        assert_eq!(shell_single_quote("plain"), "'plain'");
        // an embedded single quote becomes the '\'' splice
        assert_eq!(shell_single_quote("a'b"), "'a'\\''b'");
        // shell metacharacters end up inside one quoted literal (no injection)
        for payload in ["; rm -rf /", "$(whoami)", "`id`", "&& curl evil"] {
            let q = shell_single_quote(payload);
            assert!(q.starts_with('\'') && q.ends_with('\''));
            assert_eq!(&q[1..q.len() - 1], payload, "payload kept verbatim, quoted");
        }
    }

    #[test]
    fn ssh_search_argv_tailscale_form() {
        let (prog, args) = ssh_search_argv(
            &SshTransport::Tailscale,
            "switchblade",
            "secret; rm -rf /",
            "/data",
            5,
        );
        assert_eq!(prog, "tailscale");
        assert_eq!(args[0], "ssh");
        assert_eq!(args[1], "switchblade");
        assert_eq!(args[2], "--");
        let cmd = &args[3];
        assert!(cmd.contains("grep -rIl --"), "{cmd}");
        assert!(
            cmd.contains("'secret; rm -rf /'"),
            "query single-quoted: {cmd}"
        );
        assert!(cmd.contains("'/data'"));
        assert!(cmd.contains("head -n 5"));
    }

    #[test]
    fn ssh_search_argv_plain_form() {
        let (prog, args) = ssh_search_argv(
            &SshTransport::Plain { port: 8022 },
            "pixel-10-pro-xl",
            "tailscale",
            "/sdcard",
            3,
        );
        assert_eq!(prog, "ssh");
        assert_eq!(args[0], "-p");
        assert_eq!(args[1], "8022");
        assert!(args.contains(&"BatchMode=yes".to_string()));
        assert_eq!(args[args.len() - 3], "pixel-10-pro-xl");
        assert_eq!(args[args.len() - 2], "--");
        assert!(args.last().unwrap().contains("'tailscale'"));
    }

    #[test]
    fn ssh_cat_argv_escapes_path() {
        let (prog, args) = ssh_cat_argv(
            &SshTransport::Plain { port: 22 },
            "host",
            "/weird path/$(x).txt",
        );
        assert_eq!(prog, "ssh");
        let cmd = args.last().unwrap();
        assert!(cmd.starts_with("cat -- '"), "{cmd}");
        assert!(cmd.contains("'/weird path/$(x).txt'"), "{cmd}");
    }

    #[test]
    fn parse_status_devices_reads_online_offline() {
        // The captured real `tailscale status` sample from this machine.
        let sample = "100.100.225.71   pixel-10-pro-xl  crussella0129@  android  -\n\
                      100.98.104.44    switchblade      crussella0129@  linux    offline, last seen 1h ago\n";
        let devs = parse_status_devices(sample);
        assert_eq!(devs.len(), 2);
        let pixel = devs.iter().find(|d| d.name == "pixel-10-pro-xl").unwrap();
        assert_eq!(pixel.ip, "100.100.225.71");
        assert!(pixel.online, "pixel has no offline marker → online");
        let sb = devs.iter().find(|d| d.name == "switchblade").unwrap();
        assert!(!sb.online, "switchblade marked offline");
    }

    #[test]
    fn tailnet_retriever_plane_label() {
        let r = TailnetFsRetriever::new("switchblade", "/data", SshTransport::Tailscale);
        assert_eq!(r.plane(), "tailnet");
    }
}

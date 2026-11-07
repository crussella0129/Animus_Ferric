//! Fixtures shared by the server lifecycle, registration and resolution test
//! modules.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

use super::ServerRunfile;
use crate::server_process::{ProcessIdentity, canonical_test_start_token};
use crate::server_registration::{
    NativePersistenceEffects, PersistenceEffects, PersistencePhase, StagePersistError,
};

pub(crate) fn fixture_executable() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(r"C:\fixture\llama-server.exe")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/fixture/llama-server")
    }
}

pub(crate) fn process_identity(seed: u64) -> ProcessIdentity {
    ProcessIdentity {
        start_token: canonical_test_start_token(seed),
        executable: fixture_executable(),
        argv: vec!["llama-server".to_string(), "--serve".to_string()],
    }
}

/// The opaque `tailscale_serve` object a record written by the retired
/// `server up --tailscale` carried; only its presence matters now.
pub(crate) fn retired_tailscale_serve() -> serde_json::Value {
    serde_json::json!({
        "mount_path": "/_ferric/00112233445566778899aabbccddeeff",
        "remote_base_url": "https://example-host.tailnet-example.ts.net/_ferric/00112233445566778899aabbccddeeff/v1",
        "apply_confirmed": true
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PersistenceEvent {
    pub(crate) phase: PersistencePhase,
    pub(crate) final_path: PathBuf,
    pub(crate) stage_path: Option<PathBuf>,
    pub(crate) byte_len: Option<usize>,
}

type PhaseHook = Box<dyn FnMut(PersistencePhase, &Path)>;

/// Native persistence that records every phase and can fail one exact
/// phase of one destination, or hard-link a committed stage instead of
/// moving it so stage cleanup has something to do.
#[derive(Default)]
pub(crate) struct ScriptedPersistenceEffects {
    pub(crate) events: Vec<PersistenceEvent>,
    pub(crate) serializations: usize,
    failure: Option<(PathBuf, PersistencePhase)>,
    retain_stage_after_persist: Option<PathBuf>,
    on_phase: Option<PhaseHook>,
}

impl ScriptedPersistenceEffects {
    pub(crate) fn failing(final_path: &Path, phase: PersistencePhase) -> Self {
        Self {
            failure: Some((final_path.to_path_buf(), phase)),
            ..Self::default()
        }
    }

    pub(crate) fn retaining_committed_stage(final_path: &Path) -> Self {
        Self {
            retain_stage_after_persist: Some(final_path.to_path_buf()),
            ..Self::default()
        }
    }

    /// Also report each recorded phase to `hook`, so a caller can interleave
    /// persistence with its own lifecycle ledger.
    pub(crate) fn on_phase(mut self, hook: impl FnMut(PersistencePhase, &Path) + 'static) -> Self {
        self.on_phase = Some(Box::new(hook));
        self
    }

    fn fails(&self, final_path: &Path, phase: PersistencePhase) -> bool {
        self.failure
            .as_ref()
            .is_some_and(|(target, target_phase)| target == final_path && *target_phase == phase)
    }

    fn record(
        &mut self,
        phase: PersistencePhase,
        final_path: &Path,
        stage_path: Option<&Path>,
        byte_len: Option<usize>,
    ) {
        if let Some(hook) = &mut self.on_phase {
            hook(phase, final_path);
        }
        self.events.push(PersistenceEvent {
            phase,
            final_path: final_path.to_path_buf(),
            stage_path: stage_path.map(Path::to_path_buf),
            byte_len,
        });
    }

    fn injected(phase: PersistencePhase) -> io::Error {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("injected {phase:?} failure"),
        )
    }
}

impl PersistenceEffects for ScriptedPersistenceEffects {
    fn serialize(&mut self, runfile: &ServerRunfile) -> serde_json::Result<Vec<u8>> {
        self.serializations += 1;
        serde_json::to_vec_pretty(runfile)
    }

    fn create_stage(&mut self, final_path: &Path, parent: &Path) -> io::Result<NamedTempFile> {
        if self.fails(final_path, PersistencePhase::CreateStage) {
            self.record(PersistencePhase::CreateStage, final_path, None, None);
            return Err(Self::injected(PersistencePhase::CreateStage));
        }
        let stage = NativePersistenceEffects.create_stage(final_path, parent)?;
        self.record(
            PersistencePhase::CreateStage,
            final_path,
            Some(stage.path()),
            None,
        );
        Ok(stage)
    }

    fn write_all(
        &mut self,
        final_path: &Path,
        stage: &mut NamedTempFile,
        raw: &[u8],
    ) -> io::Result<()> {
        self.record(
            PersistencePhase::WriteAll,
            final_path,
            Some(stage.path()),
            Some(raw.len()),
        );
        if self.fails(final_path, PersistencePhase::WriteAll) {
            let short = raw.len().min(7);
            stage.write_all(&raw[..short])?;
            return Err(Self::injected(PersistencePhase::WriteAll));
        }
        NativePersistenceEffects.write_all(final_path, stage, raw)
    }

    fn flush(&mut self, final_path: &Path, stage: &mut NamedTempFile) -> io::Result<()> {
        self.record(
            PersistencePhase::Flush,
            final_path,
            Some(stage.path()),
            None,
        );
        if self.fails(final_path, PersistencePhase::Flush) {
            return Err(Self::injected(PersistencePhase::Flush));
        }
        NativePersistenceEffects.flush(final_path, stage)
    }

    fn sync_file(&mut self, final_path: &Path, stage: &NamedTempFile) -> io::Result<()> {
        self.record(
            PersistencePhase::FileSync,
            final_path,
            Some(stage.path()),
            None,
        );
        if self.fails(final_path, PersistencePhase::FileSync) {
            return Err(Self::injected(PersistencePhase::FileSync));
        }
        NativePersistenceEffects.sync_file(final_path, stage)
    }

    fn persist_noclobber(
        &mut self,
        final_path: &Path,
        mut stage: NamedTempFile,
    ) -> Result<(), StagePersistError> {
        let stage_path = stage.path().to_path_buf();
        self.record(
            PersistencePhase::PersistNoClobber,
            final_path,
            Some(&stage_path),
            None,
        );
        if self.fails(final_path, PersistencePhase::PersistNoClobber) {
            return Err(StagePersistError {
                error: Self::injected(PersistencePhase::PersistNoClobber),
                stage,
            });
        }
        if self
            .retain_stage_after_persist
            .as_ref()
            .is_some_and(|target| target == final_path)
        {
            if let Err(error) = fs::hard_link(&stage_path, final_path) {
                return Err(StagePersistError { error, stage });
            }
            stage.disable_cleanup(true);
            drop(stage);
            return Ok(());
        }
        NativePersistenceEffects.persist_noclobber(final_path, stage)
    }

    fn sync_parent(&mut self, final_path: &Path, parent: &Path) -> io::Result<()> {
        self.record(PersistencePhase::ParentSync, final_path, None, None);
        if self.fails(final_path, PersistencePhase::ParentSync) {
            return Err(Self::injected(PersistencePhase::ParentSync));
        }
        NativePersistenceEffects.sync_parent(final_path, parent)
    }
}

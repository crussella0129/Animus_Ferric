use std::collections::VecDeque;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ferric_process::ProcessTree;

use super::{StartupError, check_cancelled};
use crate::server_process::{ListenerState, LiveProcess, ProcessIdentity};

const LOG_LIMIT: usize = 16 * 1024;

struct Tail {
    bytes: Arc<Mutex<VecDeque<u8>>>,
    reader: Option<JoinHandle<std::io::Result<()>>>,
}

impl Tail {
    fn new(mut pipe: impl Read + Send + 'static) -> Result<Self, StartupError> {
        let bytes = Arc::new(Mutex::new(VecDeque::with_capacity(LOG_LIMIT)));
        let capture = Arc::clone(&bytes);
        let reader = std::thread::Builder::new()
            .name("ferric-engine-log".into())
            .spawn(move || {
                let mut buffer = [0_u8; 4096];
                loop {
                    let count = pipe.read(&mut buffer)?;
                    if count == 0 {
                        return Ok(());
                    }
                    let mut tail = capture.lock().unwrap_or_else(|error| error.into_inner());
                    for byte in &buffer[..count] {
                        if tail.len() == LOG_LIMIT {
                            tail.pop_front();
                        }
                        tail.push_back(*byte);
                    }
                }
            })
            .map_err(|_| StartupError::resource("Engine diagnostics could not start."))?;
        Ok(Self {
            bytes,
            reader: Some(reader),
        })
    }

    fn text(&self) -> String {
        let bytes: Vec<u8> = self
            .bytes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .copied()
            .collect();
        String::from_utf8_lossy(&bytes)
            .chars()
            .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
            .collect()
    }

    fn finish(&mut self) {
        if let Some(reader) = self.reader.take() {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !reader.is_finished() {
                if Instant::now() >= deadline {
                    ferric_process::abort_on_cleanup_failure(
                        "engine diagnostics did not finish after process cleanup",
                        "reader deadline",
                    );
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            // is_finished proves join cannot wait for a still-running reader.
            if reader.join().is_err() {
                ferric_process::abort_on_cleanup_failure(
                    "engine diagnostic reader panicked",
                    "reader failure",
                );
            }
        }
    }
}

pub(super) struct ChildOwner {
    pub(super) tree: ProcessTree,
    stdout: Option<Tail>,
    stderr: Option<Tail>,
}

impl ChildOwner {
    pub(super) fn spawn(mut command: Command) -> Result<Self, StartupError> {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let tree = ProcessTree::spawn(&mut command).map_err(|_| {
            StartupError::resource("The engine could not start. Check the installed llama-server.")
        })?;
        let mut owner = Self {
            tree,
            stdout: None,
            stderr: None,
        };
        owner.stdout = Some(Tail::new(owner.tree.take_stdout().expect("piped stdout"))?);
        owner.stderr = Some(Tail::new(owner.tree.take_stderr().expect("piped stderr"))?);
        Ok(owner)
    }

    pub(super) fn diagnostics(&self) -> String {
        let mut text = self.stdout.as_ref().map(Tail::text).unwrap_or_default();
        text.push_str(&self.stderr.as_ref().map(Tail::text).unwrap_or_default());
        text
    }

    pub(super) fn cleanup(&mut self) -> Result<(), StartupError> {
        self.tree
            .terminate_and_reap()
            .map_err(|_| StartupError::resource("Owned engine cleanup could not be proved."))?;
        if let Some(tail) = &mut self.stdout {
            tail.finish();
        }
        if let Some(tail) = &mut self.stderr {
            tail.finish();
        }
        Ok(())
    }
}

impl Drop for ChildOwner {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            ferric_process::abort_on_cleanup_failure("owned startup engine cleanup failed", error);
        }
    }
}

pub(super) struct OwnedEngine {
    pub(super) child: ChildOwner,
    pub(super) process: LiveProcess,
    pub(super) identity: ProcessIdentity,
    pub(super) port: u16,
}

impl OwnedEngine {
    pub(super) fn spawn(command: Command, port: u16) -> Result<Self, StartupError> {
        let mut child = ChildOwner::spawn(command)?;
        let process = LiveProcess::acquire_child(child.tree.child()).map_err(|_| {
            StartupError::resource("Exact ownership of the engine is unavailable on this host.")
        })?;
        if child
            .tree
            .try_wait_leader()
            .map_err(|_| StartupError::resource("The engine process could not be inspected."))?
            .is_some()
        {
            return Err(StartupError::resource("The engine exited before startup."));
        }
        let identity = process
            .inspect(port)
            .map_err(|_| {
                StartupError::resource("The retained engine identity could not be verified.")
            })?
            .identity;
        Ok(Self {
            child,
            process,
            identity,
            port,
        })
    }

    pub(super) fn listener(&self) -> Result<ListenerState, StartupError> {
        let facts = self.process.inspect(self.port).map_err(|_| {
            StartupError::resource("The owned engine exited or cannot be verified.")
        })?;
        if facts.identity != self.identity {
            return Err(StartupError::resource(
                "The engine identity changed during the session.",
            ));
        }
        Ok(facts.listener)
    }

    pub(super) fn validate(&self) -> Result<(), StartupError> {
        if self.listener()? != ListenerState::OwnedByTarget {
            return Err(StartupError::resource(
                "The engine no longer exclusively owns its loopback listener.",
            ));
        }
        Ok(())
    }
}

pub(super) fn version(
    command: Command,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<String, StartupError> {
    check_cancelled(cancel)?;
    let deadline = deadline.min(Instant::now() + super::probe::PROBE_TIMEOUT);
    version_deadline(deadline)?;
    let mut owner = ChildOwner::spawn(command)?;
    let outcome = loop {
        if let Err(error) = check_cancelled(cancel) {
            break Err(error);
        }
        if let Err(error) = version_deadline(deadline) {
            break Err(error);
        }
        let status = owner.tree.try_wait_leader();
        // A success first observed after the bound is not an on-time success.
        // Check on both sides of the process inspection scheduling window.
        if let Err(error) = version_deadline(deadline) {
            break Err(error);
        }
        match status {
            Ok(Some(status)) if status.success() => break Ok(()),
            Ok(Some(_)) => {
                break Err(StartupError::resource(
                    "The installed engine failed its version probe.",
                ));
            }
            Err(_) => {
                break Err(StartupError::resource(
                    "The engine version probe could not be inspected.",
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    owner.cleanup()?;
    outcome?;
    check_cancelled(cancel)?;
    version_deadline(deadline)?;
    let version = owner
        .diagnostics()
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("llama-server version unavailable")
        .chars()
        .take(256)
        .collect();
    Ok(version)
}

fn version_deadline(deadline: Instant) -> Result<(), StartupError> {
    if Instant::now() >= deadline {
        Err(StartupError::resource(
            "The engine version probe exceeded its deadline (at most five seconds).",
        ))
    } else {
        Ok(())
    }
}

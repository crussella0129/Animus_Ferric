//! A generation token distinguishes each registration from a reused OS ID.
use std::collections::BTreeMap;
use std::io;
#[cfg(any(unix, test))]
use std::sync::Mutex;
#[cfg(unix)]
use std::sync::{MutexGuard, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScopeKey {
    pub(crate) id: u32,
    generation: u64,
}

struct Entry {
    generation: u64,
    signalled: bool,
}

#[derive(Default)]
pub(crate) struct Registry {
    shutting_down: bool,
    generation: u64,
    scopes: BTreeMap<u32, Entry>,
}

impl Registry {
    pub(crate) fn allow_spawn(&self) -> io::Result<()> {
        if self.shutting_down || self.generation == u64::MAX {
            Err(io::Error::other(
                "process ownership is shutting down or exhausted",
            ))
        } else {
            Ok(())
        }
    }
    pub(crate) fn register(&mut self, id: u32) -> ScopeKey {
        // allow_spawn is checked before native spawn while retaining this lock.
        self.generation = self
            .generation
            .checked_add(1)
            .expect("checked registration capacity");
        let generation = self.generation;
        self.scopes.insert(
            id,
            Entry {
                generation,
                signalled: false,
            },
        );
        ScopeKey { id, generation }
    }
    pub(crate) fn contains(&self, key: ScopeKey) -> bool {
        self.scopes
            .get(&key.id)
            .is_some_and(|entry| entry.generation == key.generation)
    }
    pub(crate) fn signal_once(
        &mut self,
        key: ScopeKey,
        signal: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        let Some(entry) = self
            .scopes
            .get_mut(&key.id)
            .filter(|entry| entry.generation == key.generation)
        else {
            return Ok(());
        };
        if !entry.signalled {
            signal()?;
            entry.signalled = true;
        }
        Ok(())
    }
    pub(crate) fn keys(&self) -> Vec<ScopeKey> {
        self.scopes
            .iter()
            .map(|(&id, entry)| ScopeKey {
                id,
                generation: entry.generation,
            })
            .collect()
    }
    pub(crate) fn remove_if_absent(
        &mut self,
        key: ScopeKey,
        absent: impl FnOnce() -> io::Result<bool>,
    ) -> io::Result<bool> {
        // A replacement registration proves OS reuse; stale owners must never
        // signal, wait/reap, probe, or remove anything through that number.
        if !self.contains(key) {
            return Ok(true);
        }
        if absent()? {
            self.scopes.remove(&key.id);
            Ok(true)
        } else {
            Ok(false)
        }
    }
    pub(crate) fn begin_shutdown(&mut self) {
        self.shutting_down = true;
    }
    #[cfg(test)]
    pub(crate) fn signal_registered(&self, mut signal: impl FnMut(u32)) {
        for id in self.scopes.keys().copied() {
            signal(id);
        }
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }
}

#[cfg(unix)]
pub(crate) fn lock() -> MutexGuard<'static, Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| Mutex::new(Registry::default()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[test]
fn shutdown_registry_rejects_late_spawn() {
    use std::sync::Arc;
    let registry = Arc::new(Mutex::new(Registry::default()));
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let key = registry.lock().unwrap().register(42);
    let cleanup = Arc::clone(&registry);
    let cleanup_thread = std::thread::spawn(move || {
        cleanup
            .lock()
            .unwrap()
            .remove_if_absent(key, || {
                entered_tx.send(()).unwrap();
                release_rx
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap();
                Ok(true)
            })
            .unwrap();
    });
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    let shutdown = Arc::clone(&registry);
    let shutdown_thread = std::thread::spawn(move || {
        let mut registry = shutdown.lock().unwrap();
        registry.begin_shutdown();
        let mut signalled = Vec::new();
        registry.signal_registered(|id| signalled.push(id));
        assert!(
            signalled.is_empty(),
            "removed scope reached stale signal recorder"
        );
        assert!(registry.allow_spawn().is_err());
        assert!(registry.is_empty());
    });
    release_tx.send(()).unwrap();
    cleanup_thread.join().unwrap();
    shutdown_thread.join().unwrap();

    let mut registry = Registry::default();
    let key = registry.register(7);
    assert!(registry.contains(key));
    let mut attempts = 0;
    registry
        .signal_once(key, || {
            attempts += 1;
            Ok(())
        })
        .unwrap();
    registry
        .signal_once(key, || {
            attempts += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(attempts, 1);
    assert_eq!(registry.keys(), [key]);
    registry.begin_shutdown();
    registry.remove_if_absent(key, || Ok(true)).unwrap();
    registry.signal_registered(|id| panic!("removed scope {id} signalled twice"));
    assert!(registry.allow_spawn().is_err());
}

#[test]
fn recycled_id_cannot_redirect_stale_scope_operations() {
    let mut registry = Registry::default();
    let old = registry.register(42);
    registry.signal_once(old, || Ok(())).unwrap();
    let replacement = registry.register(42);
    assert_ne!(old, replacement);
    assert!(!registry.contains(old));
    registry
        .signal_once(old, || panic!("stale scope signalled a replacement"))
        .unwrap();
    registry
        .remove_if_absent(old, || panic!("stale scope probed or reaped a replacement"))
        .unwrap();
    assert!(
        registry.contains(replacement),
        "stale removal erased new ownership"
    );
    let mut signals = 0;
    registry
        .signal_once(replacement, || {
            signals += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(signals, 1);
    assert_eq!(registry.keys(), [replacement]);
}

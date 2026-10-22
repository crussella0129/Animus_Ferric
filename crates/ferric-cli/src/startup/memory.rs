//! Best-effort system-memory probe for the front door's hardware-fit signal.
//! `None` ("unknown") is a valid outcome — a locked-down container or an
//! unreadable `/proc` must never yield a fabricated number. The native probe
//! lives in `ferric_process::memory` (Sprint 126, T-12608); this module keeps
//! the front door's injectable seam.

pub(crate) use ferric_process::memory::SystemMemory;

/// Injectable seam (mirrors `HumanIo`/`Preparation`) so the front-door surfaces
/// stay testable without real hardware.
pub(crate) trait MemoryProbe {
    fn probe(&self) -> Option<SystemMemory>;
}

/// The real probe used outside tests.
pub(crate) struct NativeMemoryProbe;

impl MemoryProbe for NativeMemoryProbe {
    fn probe(&self) -> Option<SystemMemory> {
        ferric_process::memory::system_memory()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The probe is implemented only for Linux and Windows; assert the real read
    // works where it exists (both CI gates, and the dev host).
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn native_probe_reports_positive_total() {
        let mem = NativeMemoryProbe
            .probe()
            .expect("a supported host reports memory");
        assert!(mem.total_bytes > 0);
        assert!(mem.available_bytes > 0);
    }
}

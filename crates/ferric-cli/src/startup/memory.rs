//! Best-effort system-memory probe for the front door's hardware-fit signal.
//! `None` ("unknown") is a valid outcome — a locked-down container or an
//! unreadable `/proc` must never yield a fabricated number.

/// Total and currently-available physical memory, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SystemMemory {
    pub(crate) total_bytes: u64,
    pub(crate) available_bytes: u64,
}

/// Injectable seam (mirrors `HumanIo`/`Preparation`) so the front-door surfaces
/// stay testable without real hardware.
pub(crate) trait MemoryProbe {
    fn probe(&self) -> Option<SystemMemory>;
}

/// The real probe used outside tests.
pub(crate) struct NativeMemoryProbe;

impl MemoryProbe for NativeMemoryProbe {
    fn probe(&self) -> Option<SystemMemory> {
        native_probe()
    }
}

#[cfg(target_os = "linux")]
fn native_probe() -> Option<SystemMemory> {
    parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

#[cfg(target_os = "windows")]
fn native_probe() -> Option<SystemMemory> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: MEMORYSTATUSEX is plain-old-data; we set dwLength and the kernel
    // fills the rest. A zero return is failure -> None; no resource is retained.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return None;
    }
    Some(SystemMemory {
        total_bytes: status.ullTotalPhys,
        available_bytes: status.ullAvailPhys,
    })
}

// macOS lacks a cheap `MemAvailable` analogue (it needs mach `vm_statistics`), so
// rather than pass total off as available it reports Unknown until that probe is
// built; every other target is Unknown too. Deferred with T-11507's GPU fit.
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn native_probe() -> Option<SystemMemory> {
    None
}

/// Parse `/proc/meminfo`. Pure, so it is unit-testable without the file. Values
/// in the file are in kB; converted to bytes. Both `MemTotal` and `MemAvailable`
/// are required — a kernel too old for `MemAvailable` yields `None`, not a guess.
#[cfg(any(target_os = "linux", test))]
fn parse_meminfo(text: &str) -> Option<SystemMemory> {
    let mut total = None;
    let mut available = None;
    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let kb = rest
            .split_whitespace()
            .next()
            .and_then(|n| n.parse::<u64>().ok());
        match key {
            "MemTotal" => total = kb.map(|k| k.saturating_mul(1024)),
            "MemAvailable" => available = kb.map(|k| k.saturating_mul(1024)),
            _ => {}
        }
    }
    Some(SystemMemory {
        total_bytes: total?,
        available_bytes: available?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_meminfo_reads_total_and_available() {
        let text = "MemTotal:       16342184 kB\nMemFree:         1000000 kB\nMemAvailable:    8000000 kB\n";
        let mem = parse_meminfo(text).expect("standard meminfo parses");
        assert_eq!(mem.total_bytes, 16_342_184 * 1024);
        assert_eq!(mem.available_bytes, 8_000_000 * 1024);
    }

    #[test]
    fn parse_meminfo_missing_available_is_none() {
        let text = "MemTotal:       16342184 kB\nMemFree:         1000000 kB\n";
        assert!(parse_meminfo(text).is_none());
    }

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

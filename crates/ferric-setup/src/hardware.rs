//! What this machine is, and which model fits it.

use crate::catalog::{ModelEntry, find_model};

const GIB: u64 = 1 << 30;

/// The facts the recommendation uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hardware {
    pub os: &'static str,
    pub arch: &'static str,
    /// Total physical memory, when the platform probe can read it.
    pub total_memory: Option<u64>,
}

impl Hardware {
    pub fn detect() -> Self {
        Hardware {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            total_memory: ferric_process::memory::system_memory().map(|memory| memory.total_bytes),
        }
    }

    /// The model to offer by default.
    ///
    /// Engines start on the CPU with a 4096-token context (Ferric's launch
    /// defaults), so this is about memory and speed, not GPU size. Under 12
    /// GiB of RAM, 7B at Q4 (about 5.5 GiB working set) competes with everything
    /// else on the machine, so the 1.5B model is offered. At 12 GiB or more,
    /// or when memory is unknown (macOS), the 7B model is offered. 14B is
    /// never the default on CPU: it is accurate but slow.
    pub fn recommended_model(&self) -> &'static ModelEntry {
        let id = match self.total_memory {
            Some(total) if total < 12 * GIB => "1.5b",
            _ => "7b",
        };
        find_model(id).expect("catalog holds the recommended ids")
    }

    pub fn memory_label(&self) -> String {
        match self.total_memory {
            Some(total) => format!("{:.0} GiB RAM", total as f64 / GIB as f64),
            None => "memory unknown".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_memory(total: Option<u64>) -> Hardware {
        Hardware {
            os: "linux",
            arch: "x86_64",
            total_memory: total,
        }
    }

    #[test]
    fn small_machines_get_the_small_model() {
        assert_eq!(with_memory(Some(8 * GIB)).recommended_model().id, "1.5b");
        assert_eq!(with_memory(Some(11 * GIB)).recommended_model().id, "1.5b");
    }

    #[test]
    fn typical_and_unknown_machines_get_7b() {
        assert_eq!(with_memory(Some(12 * GIB)).recommended_model().id, "7b");
        assert_eq!(with_memory(Some(64 * GIB)).recommended_model().id, "7b");
        assert_eq!(with_memory(None).recommended_model().id, "7b");
    }
}

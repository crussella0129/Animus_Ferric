//! Hardware fit: is there room to run this model? A deliberately conservative,
//! honest estimate — enough to warn before the front door starts a model that
//! cannot run, never a byte-exact predictor. "Unknown" is a first-class outcome
//! so an unmeasured machine is reported as unmeasured, not guessed.

/// Extra bytes charged per context token for the KV cache. A conservative upper
/// bound across the small local models Ferric targets; the picker does not have
/// per-model layer dimensions, so we overestimate — the safe direction for a
/// "will it run" warning.
const CONTEXT_KV_BYTES_PER_TOKEN: u64 = 256 * 1024;

/// A floor for runtime + OS slack, independent of context, so a tiny model at
/// zero context still reserves headroom.
const BASE_HEADROOM_BYTES: u64 = 512 * 1024 * 1024;

/// A model is `Tight` once its estimate crosses this fraction of available
/// memory (8/10 = 80%), and `WontFit` once it exceeds available outright.
const TIGHT_NUM: u64 = 8;
const TIGHT_DEN: u64 = 10;

/// Estimated resident memory (bytes) to run a GGUF of `file_bytes` at `context`
/// tokens: the weights (loaded ~1:1 for a given quant) plus a KV-cache
/// allowance plus a base headroom floor. Always ≥ the weights, grows with
/// context, and saturates rather than overflowing on pathological input.
pub fn estimate_model_memory(file_bytes: u64, context: u32) -> u64 {
    let kv = (context as u64).saturating_mul(CONTEXT_KV_BYTES_PER_TOKEN);
    file_bytes
        .saturating_add(kv)
        .saturating_add(BASE_HEADROOM_BYTES)
}

/// How a model's estimated need compares to what the machine can give it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Comfortable margin.
    Fits,
    /// Would run but leaves little slack — likely slow or fragile.
    Tight,
    /// Estimated need exceeds available memory.
    WontFit,
    /// Available memory could not be measured; nothing is asserted.
    Unknown,
}

/// Classify `estimate` against currently-**available** memory. `None` (an
/// unreadable probe) is `Unknown` — never a fabricated pass or fail. The
/// comparison is against available, not total, on purpose: memory already held
/// by the OS and other apps cannot run the model.
pub fn classify_fit(estimate: u64, available: Option<u64>) -> Fit {
    let Some(available) = available else {
        return Fit::Unknown;
    };
    if estimate > available {
        Fit::WontFit
    } else if estimate.saturating_mul(TIGHT_DEN) > available.saturating_mul(TIGHT_NUM) {
        // estimate > 80% of available
        Fit::Tight
    } else {
        Fit::Fits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn estimate_covers_weights_and_context() {
        let file = 4 * GIB;
        // At least the weights plus a positive floor.
        assert!(estimate_model_memory(file, 0) > file);
        // Monotonic in context.
        assert!(estimate_model_memory(file, 8192) > estimate_model_memory(file, 4096));
        // Saturates instead of overflowing.
        assert_eq!(estimate_model_memory(u64::MAX, u32::MAX), u64::MAX);
    }

    #[test]
    fn classify_fits_tight_and_wontfit() {
        // Comfortable: 1 GiB estimate against 8 GiB available.
        assert_eq!(classify_fit(GIB, Some(8 * GIB)), Fit::Fits);
        // Tight: 7 GiB estimate is > 80% of 8 GiB but still fits.
        assert_eq!(classify_fit(7 * GIB, Some(8 * GIB)), Fit::Tight);
        // Won't fit: estimate exceeds available.
        assert_eq!(classify_fit(9 * GIB, Some(8 * GIB)), Fit::WontFit);
    }

    #[test]
    fn classify_none_is_unknown() {
        assert_eq!(classify_fit(GIB, None), Fit::Unknown);
    }

    #[test]
    fn human_test_27b_on_small_ram_is_wontfit() {
        // The first human use test: a 15.3 GiB 27B against a modest machine.
        let file = (15.3 * GIB as f64) as u64;
        let estimate = estimate_model_memory(file, 4096);
        assert_eq!(classify_fit(estimate, Some(8 * GIB)), Fit::WontFit);
    }
}

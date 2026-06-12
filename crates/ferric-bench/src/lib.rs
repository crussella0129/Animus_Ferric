//! The L0–L6 capability ladder (ADR-019).
//!
//! Stub in T-201; spec model, runner, verification, and calibration land in
//! T-211..T-214.

#[cfg(test)]
mod tests {
    /// ADR-016: preserve_order must be active workspace-wide — the action
    /// grammar depends on insertion-order serialization.
    #[test]
    fn preserve_order_active() {
        let mut obj = serde_json::Map::new();
        obj.insert("zzz".to_string(), serde_json::Value::Null);
        obj.insert("aaa".to_string(), serde_json::Value::Null);
        let text = serde_json::to_string(&obj).unwrap();
        assert!(
            text.find("zzz").unwrap() < text.find("aaa").unwrap(),
            "serde_json must serialize in insertion order (preserve_order)"
        );
    }
}

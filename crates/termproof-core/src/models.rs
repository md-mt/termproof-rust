//! Canonical models: `StepResult` and its serialization.

use serde::{Deserialize, Serialize};

/// The per-step engine output, byte-for-byte the Python `StepResult`.
///
/// The Rust copy lives here so both the step engine and the future
/// RunResult/evidence crates have one source of truth. Display, passed,
/// detail, and screen are the only fields; serialization must be stable JSON
/// (same keys and types as Python's `to_dict`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepResult {
    /// Human-readable display name (either `step.name` or `"{index}:{action}"`).
    pub name: String,
    /// Whether the step succeeded.
    pub passed: bool,
    /// Evidence detail (e.g. `found '...'`, `matched ... -> ...`, `pressed enter`).
    pub detail: String,
    /// Screen snapshot at step completion, r-stripped per line and without
    /// trailing blank lines (same normalization as Python's `screen_text`).
    pub screen: String,
}

#[cfg(test)]
mod tests {
    use super::StepResult;

    #[test]
    fn serde_roundtrip() {
        let r = StepResult {
            name: "1:wait_for_text".into(),
            passed: true,
            detail: "found 'hello'".into(),
            screen: "hello\n> ".into(),
        };
        let j = serde_json::to_string(&r).unwrap();
        let back: StepResult = serde_json::from_str(&j).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn to_dict_keys_match_python() {
        let r = StepResult {
            name: "n".into(),
            passed: false,
            detail: "d".into(),
            screen: "s".into(),
        };
        let v: serde_json::Value = serde_json::to_value(&r).unwrap();
        let keys: Vec<_> = v.as_object().unwrap().keys().collect();
        // serde derives sorted keys: detail, name, passed, screen
        // Python's to_dict uses the same four keys (unordered dict). Equality
        // is only about the keys existing, not order.
        for k in ["name", "passed", "detail", "screen"] {
            assert!(v.get(k).is_some(), "missing key {k}");
        }
        assert_eq!(keys.len(), 4);
    }
}

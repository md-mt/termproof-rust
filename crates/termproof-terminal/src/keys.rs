//! Canonical key normalization and escape-sequence mapping, frozen from
//! `termproof/session.py:KEYS`.
//!
//! The Python oracle maps a small fixed set of named keys to VT escape
//! sequences. `Ctrl-` prefixes are handled separately via `sendcontrol` in
//! session; here we model the same split so callers that need the raw
//! sequence and callers that need the control-char control path both match.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Fixed named-key map, byte-for-byte the Python `KEYS` dict.
pub static KEYS: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();

fn keys_map() -> &'static HashMap<&'static str, &'static str> {
    KEYS.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert("enter", "\r");
        m.insert("escape", "\x1b");
        m.insert("tab", "\t");
        m.insert("backspace", "\x7f");
        m.insert("up", "\x1b[A");
        m.insert("down", "\x1b[B");
        m.insert("right", "\x1b[C");
        m.insert("left", "\x1b[D");
        m
    })
}

/// Lowercase-normalised lookup used by `press("ENTER")` etc. Returns `None`
/// for unknown keys — callers surface a failed step with the same message as
/// the Python runner (which raises `KeyError` and the runner converts to a
/// failed `StepResult`).
pub fn normalize_key(raw: &str) -> String {
    raw.to_ascii_lowercase()
}

/// Try to turn a `press` key into the bytes the terminal expects.
///
/// Returns `Ok(PressAction)` on success. `Err(unknown)` carries the original
/// key string so step detail can echo it (matches Python `KeyError` surface).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PressAction {
    /// `Ctrl-<letter>` → single control character, sent via `sendcontrol`.
    Ctrl(char),
    /// Named key from the `KEYS` map.
    Sequence(String),
}

/// Error from `press_sequence` when a key is not in the frozen map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PressError {
    /// Key is not a known named key and not a `Ctrl-` combo.
    UnknownKey(String),
    /// `Ctrl-` prefix with empty or multi-char suffix.
    BadCtrl(String),
}

/// Resolve a key string into a `PressAction` using the frozen Python contract.
///
/// `Ctrl-` is case-insensitive and normalised to lowercase before handing to
/// `sendcontrol`; named keys are lowercased before lookup.
pub fn press_sequence(key: &str) -> Result<PressAction, PressError> {
    let normalized = normalize_key(key);
    if let Some(suffix) = normalized.strip_prefix("ctrl-") {
        // Python does `sendcontrol(normalized.removeprefix("ctrl-"))` and lets
        // pexpect validate the char. We reject empty/multi-char here so the
        // failure is deterministic without depending on pexpect internals.
        let mut chars = suffix.chars();
        match (chars.next(), chars.next()) {
            (Some(ch), None) => Ok(PressAction::Ctrl(ch)),
            _ => Err(PressError::BadCtrl(key.to_string())),
        }
    } else {
        match keys_map().get(normalized.as_str()) {
            Some(seq) => Ok(PressAction::Sequence(seq.to_string())),
            None => Err(PressError::UnknownKey(key.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_maps_to_cr() {
        assert_eq!(
            press_sequence("enter").unwrap(),
            PressAction::Sequence("\r".to_string())
        );
    }

    #[test]
    fn case_insensitive_enter() {
        assert_eq!(
            press_sequence("ENTER").unwrap(),
            PressAction::Sequence("\r".to_string())
        );
    }

    #[test]
    fn ctrl_c_lower() {
        assert_eq!(press_sequence("ctrl-c").unwrap(), PressAction::Ctrl('c'));
    }

    #[test]
    fn ctrl_prefix_case_insensitive() {
        assert_eq!(press_sequence("Ctrl-C").unwrap(), PressAction::Ctrl('c'));
    }

    #[test]
    fn unknown_key_errors() {
        assert!(matches!(
            press_sequence("f13"),
            Err(PressError::UnknownKey(_))
        ));
    }
}

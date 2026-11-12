//! Legacy Shift-selection keys enter the canonical layer exactly once.
//!
//! The old Boolean forced ownership even when programs requested capture.
//! Mapping it to never/always preserves that behavior, whereas an inverted
//! Boolean would incorrectly allow XTSHIFTESCAPE to override an old config.
//! Normalization happens at admission, before any layer map is updated.
//! No alias remains in persistence, and the typed policy needs no string lookup
//! on pointer dispatch. Reset and override queries use the same canonical key.
use super::{ApplyReport, Layer, SettingsStore};
use puzzle_bus::{Value, value::ValidationIssue};

/// Both historic spellings refer to the same canonical layer entry.
pub(super) fn canonical(key: &str) -> &str {
    match key {
        "mouse.shiftSelects" | "mouse_shift_selects" => "mouse.shiftCapture",
        _ => key,
    }
}

impl SettingsStore {
    /// Normalizes a legacy write or leaves unrelated keys to the schema.
    pub(super) fn apply_mouse_alias(
        &mut self,
        key: &str,
        value: &Value,
        layer: Layer,
    ) -> Option<ApplyReport> {
        // Unrelated fields retain the normal schema validation/reporting path.
        if canonical(key) == key {
            return None;
        }
        // Validate the old type before translating: strings were never valid here.
        let Value::Bool(selects) = value else {
            // Report the spelling the caller supplied, rather than a renamed key.
            let message = "expected a boolean for legacy Shift selection".to_string();
            tracing::warn!(event = "settings.invalid", key, %message);
            return Some(ApplyReport {
                rejected: vec![ValidationIssue {
                    path: key.into(),
                    message,
                }],
                ..ApplyReport::default()
            });
        };
        // Only canonical entries persist, so reset and precedence have one owner.
        // Re-enter ordinary admission to validate and store the translated value.
        // Always/never encode the old forced policy, not an overridable default.
        Some(self.apply_key(
            canonical(key),
            Value::String(if *selects { "never" } else { "always" }.into()),
            layer,
        ))
    }
}

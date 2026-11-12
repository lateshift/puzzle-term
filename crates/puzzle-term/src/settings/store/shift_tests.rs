//! Legacy Shift configuration remains forced while the new Boolean is a default.
//!
//! These tests use the same store admission surface as JSON, CLI and host config.
//! Compatibility must survive a write/export/reload cycle: otherwise an old
//! forced policy would become a program-overridable preference after restart.
//! Both legacy spellings address one canonical entry, so layer precedence and
//! reset cannot leave a hidden alias that resurfaces on the next recomputation.
//! XTSHIFTESCAPE is deliberately absent here: Session tests cover negotiation;
//! these tests isolate what is persisted before a program can send any bytes.
use super::*;
use crate::settings::ShiftCapture;

/// Aliases participate in one canonical layer, including export and reset.
#[test]
fn shift_capture_aliases_and_layers() {
    for alias in ["mouse.shiftSelects", "mouse_shift_selects"] {
        let mut store = SettingsStore::default();
        // The new default permits negotiation; it is not the old forced true.
        assert_eq!(store.effective().mouse_shift_capture, ShiftCapture::False);
        for (selects, expected) in [(true, ShiftCapture::Never), (false, ShiftCapture::Always)] {
            // Both old values must preserve unconditional ownership.
            // Mapping true to the new false default would allow program capture.
            let report = store.apply_key(alias, Value::Bool(selects), Layer::User);
            assert_eq!(report.applied, ["mouse.shiftCapture"]);
            // Recognized aliases must not be preserved as unknown settings.
            assert!(report.unknown.is_empty());
            assert_eq!(store.effective().mouse_shift_capture, expected);
            assert!(store.has_override(alias));
            // Canonical export avoids retaining a second stale alias after a reload.
            let mut restored = SettingsStore::default();
            assert!(
                restored
                    .replace_user(&store.export_user())
                    .rejected
                    .is_empty()
            );
            assert_eq!(restored.effective().mouse_shift_capture, expected);
            // A session override wins over the legacy user's normalized entry.
            store.apply_key("mouse.shiftCapture", Value::Bool(true), Layer::Session);
            assert_eq!(store.effective().mouse_shift_capture, ShiftCapture::True);
            // Reset through the alias removes that same higher-precedence entry.
            // The surviving user value distinguishes reset from writing defaults.
            store.reset(alias, Layer::Session);
            assert_eq!(store.effective().mouse_shift_capture, expected);
        }
        // Removing the user alias reveals the schema default, not another copy.
        store.reset(alias, Layer::User);
        assert!(!store.has_override("mouse.shiftCapture"));
        assert_eq!(store.effective().mouse_shift_capture, ShiftCapture::False);
    }
}

/// Invalid compatibility values leave the last accepted policy untouched.
#[test]
fn shift_capture_invalid_values_preserve_policy() {
    for alias in ["mouse.shiftSelects", "mouse_shift_selects"] {
        let mut store = SettingsStore::default();
        store.apply_key(alias, Value::Bool(false), Layer::User);
        // The old schema accepted Booleans only; a truthy string is not valid.
        // Keep a nondefault policy installed to detect destructive rejection.
        assert_eq!(
            store
                .apply_key(alias, Value::String("true".into()), Layer::User)
                .rejected
                .len(),
            1
        );
        assert_eq!(store.effective().mouse_shift_capture, ShiftCapture::Always);
        store.reset(alias, Layer::User);
        // New Boolean defaults also reject coercion and misspelled strings.
        // Null is not reset, numeric one is not true, and string false is not false.
        for invalid in [
            Value::Null,
            Value::String("false".into()),
            Value::Number(1.0),
        ] {
            assert_eq!(
                store
                    .apply_key("mouse.shiftCapture", invalid, Layer::Session)
                    .rejected
                    .len(),
                1
            );
            // Rejection must not install a new higher-precedence override.
            // This checks the observable effective value after each bad write.
            assert_eq!(store.effective().mouse_shift_capture, ShiftCapture::False);
        }
    }
}

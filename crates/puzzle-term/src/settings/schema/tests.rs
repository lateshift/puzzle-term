//! Generated schema invariants for every declaration in §14.5.
use super::{Settings, validate};
use crate::settings::BlinkMode;
use puzzle_bus::Value;
use std::collections::BTreeSet;

#[test]
/// Defaults, metadata and validators come from the same macro entry.
fn every_default_validates_and_has_documentation() {
    let schema = Settings::schema();
    let mut keys = BTreeSet::new();
    for definition in schema {
        assert!(!definition.doc.trim().is_empty(), "{}", definition.key);
        assert!(keys.insert(definition.key), "duplicate {}", definition.key);
        let checked = validate(definition.default.clone(), definition.validator);
        assert!(checked.is_ok(), "default failed for {}", definition.key);
        let mut settings = Settings::default();
        assert!(settings.assign(definition.key, definition.default).is_ok());
    }
    assert!(keys.contains("font.size"));
    assert!(keys.contains("theme.mode"));
    assert!(keys.contains("scrollback.lines"));
}

#[test]
/// Cursor defaults follow §14.2, including the mixed string/bool blink type.
fn cursor_catalogue_matches_precedence_settings() {
    let settings = Settings::default();
    assert_eq!(settings.cursor_blink, BlinkMode::Auto);
    assert_eq!(settings.cursor_style, "block");
    assert_eq!(settings.cursor_color, "theme");
    assert!(settings.cursor_allow_app_style);
    for value in [
        Value::String("auto".into()),
        Value::Bool(true),
        Value::Bool(false),
    ] {
        let mut copy = settings.clone();
        assert!(copy.assign("cursor.blink", value).is_ok());
    }
    let mut copy = settings.clone();
    assert!(
        copy.assign("cursor.blink", Value::String("yes".into()))
            .is_err()
    );
    assert!(
        copy.assign("cursor.color", Value::String("#abcdef".into()))
            .is_ok()
    );
    assert!(
        copy.assign("cursor.color", Value::String("#abcd".into()))
            .is_err()
    );
}

/// Injection choices validate once and are reported for the next native session.
#[test]
#[allow(
    clippy::expect_used,
    reason = "the declared setting is required fixture metadata"
)]
fn shell_integration_setting() {
    use crate::settings::{ApplyMode, Layer, Platform, SettingsStore};
    let definition = Settings::schema()
        .into_iter()
        .find(|d| d.key == "shellIntegration")
        .expect("setting");
    assert_eq!(definition.apply, ApplyMode::NextSession);
    assert_eq!(definition.platform, Platform::Desktop);
    let mut store = SettingsStore::default();
    assert_eq!(store.effective().shell_integration, "detect");
    for choice in ["none", "bash", "zsh", "fish", "detect"] {
        let report = store.apply_key(
            "shellIntegration",
            Value::String(choice.into()),
            Layer::Session,
        );
        assert!(report.rejected.is_empty());
        assert_eq!(report.next_session, ["shellIntegration"]);
        assert_eq!(store.effective().shell_integration, choice);
    }
    let rejected = store.apply_key(
        "shellIntegration",
        Value::String("guess".into()),
        Layer::Session,
    );
    assert_eq!(rejected.rejected.len(), 1);
    assert_eq!(store.effective().shell_integration, "detect");
}

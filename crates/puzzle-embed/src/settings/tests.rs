//! Native file and CLI settings behavior (§14.4).
//!
//! Each test owns a unique temporary directory so a user's real config
//! cannot affect acceptance and files are removed after the assertion.
#![allow(
    clippy::unwrap_used,
    reason = "test setup and assertions need explicit failure"
)]
use super::DesktopSettings;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

/// Distinguishes parallel tests in the same process without a new crate.
static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

/// One self-cleaning config directory under the process temp root.
struct TempConfig {
    /// Directory whose children belong exclusively to this test.
    directory: PathBuf,
    /// User config path used by the adapter.
    path: PathBuf,
}

impl TempConfig {
    /// Creates a unique path before any config file is written.
    fn new() -> Self {
        let ordinal = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("puzzle-settings-{}-{ordinal}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("config.json");
        Self { directory, path }
    }

    /// Supplies a UTF-8 path to the native command parser.
    fn text(&self) -> &str {
        self.path.to_str().unwrap()
    }
}

impl Drop for TempConfig {
    /// A failed test still leaves no stray crate-local target directory.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
/// The run-only size differs from the schema until a source chooses it.
fn run_default_and_starter_schema() {
    let temp = TempConfig::new();
    let (settings, warnings) = DesktopSettings::open(Some(temp.text()), None, &[]).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(settings.store.effective().font_size, 14.0);
    assert_eq!(settings.run_font_size(), 15.0);
    DesktopSettings::init_config(Some(temp.text())).unwrap();
    let (settings, warnings) = DesktopSettings::open(Some(temp.text()), None, &[]).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(settings.run_font_size(), 15.0);
    assert!(temp.directory.join("settings.schema.json").exists());
}

#[test]
/// A malformed edit leaves the last good user and session layers visible.
fn invalid_reload_keeps_previous_values() {
    let temp = TempConfig::new();
    fs::write(&temp.path, r#"{"font":{"size":18}}"#).unwrap();
    let (mut settings, warnings) =
        DesktopSettings::open(Some(temp.text()), None, &["font.size=20".into()]).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(settings.store.effective().font_size, 20.0);
    fs::write(&temp.path, "{broken").unwrap();
    assert!(settings.reload().is_err());
    assert_eq!(settings.store.effective().font_size, 20.0);
    fs::write(&temp.path, r#"{"font":{"size":19}}"#).unwrap();
    settings.reload().unwrap();
    assert_eq!(settings.store.effective().font_size, 20.0);
    let (defaults, warnings) = DesktopSettings::open(Some(temp.text()), None, &[]).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(defaults.store.effective().font_size, 19.0);
}

#[test]
/// Unknown nested keys survive a pretty-printed native save.
fn unknown_values_round_trip() {
    let temp = TempConfig::new();
    fs::write(
        &temp.path,
        r#"{"font":{"size":17},"future":{"shape":"squircle"}}"#,
    )
    .unwrap();
    let (mut settings, _) = DesktopSettings::open(Some(temp.text()), None, &[]).unwrap();
    settings.save_user().unwrap();
    let saved: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&temp.path).unwrap()).unwrap();
    assert_eq!(saved["future"]["shape"], "squircle");
    assert_eq!(saved["font"]["size"], 17.0);
}

#[test]
/// User, profile and CLI layers resolve in that order from one JSON file.
fn profile_and_set_precedence() {
    let temp = TempConfig::new();
    fs::write(
        &temp.path,
        r#"{"font":{"size":16},"profiles":{"present":{"font":{"size":22}}}}"#,
    )
    .unwrap();
    let (settings, _) =
        DesktopSettings::open(Some(temp.text()), Some("present"), &["font.size=24".into()])
            .unwrap();
    assert_eq!(settings.store.effective().font_size, 24.0);
    assert_eq!(settings.store.active_profile(), Some("present"));
    assert!(DesktopSettings::open(Some(temp.text()), None, &["font.size=bad".into()]).is_err());
}

#[test]
/// Zoom, profile and appearance actions survive reload without rewriting config.
fn binding_host_actions_preserve_layers_and_zoom_base() {
    use puzzle_term::{keybind::Action, settings::ThemeMode};
    let temp = TempConfig::new();
    let original = r#"{"font":{"size":18},"profiles":{"review":{"font":{"size":22}}}}"#;
    fs::write(&temp.path, original).unwrap();
    let (mut settings, _) = DesktopSettings::open(Some(temp.text()), None, &[]).unwrap();
    settings.binding_action(&Action::Zoom(2), true).unwrap();
    assert_eq!(settings.run_font_size(), 20.0);
    settings
        .binding_action(&Action::Profile("review".into()), true)
        .unwrap();
    assert_eq!(settings.run_font_size(), 24.0);
    settings.binding_action(&Action::ZoomReset, true).unwrap();
    assert_eq!(settings.run_font_size(), 22.0);
    settings
        .binding_action(&Action::ToggleAppearance, true)
        .unwrap();
    assert_eq!(settings.store.effective().theme_mode, ThemeMode::Light);
    settings.reload().unwrap();
    assert_eq!(settings.store.effective().theme_mode, ThemeMode::Light);
    assert_eq!(settings.run_font_size(), 22.0);
    settings
        .binding_action(&Action::SetAppearance(ThemeMode::Auto), false)
        .unwrap();
    assert_eq!(settings.store.effective().theme_mode, ThemeMode::Auto);
    assert!(
        settings
            .binding_action(&Action::Profile("missing".into()), true)
            .is_err()
    );
    assert_eq!(settings.store.active_profile(), Some("review"));
    settings
        .binding_action(&Action::Zoom(i32::MAX), true)
        .unwrap();
    assert_eq!(settings.run_font_size(), 200.0);
    settings
        .binding_action(&Action::Zoom(i32::MIN), true)
        .unwrap();
    assert_eq!(settings.run_font_size(), 4.0);
    assert_eq!(fs::read_to_string(&temp.path).unwrap(), original);
}

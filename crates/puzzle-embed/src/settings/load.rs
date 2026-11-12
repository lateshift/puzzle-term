//! Startup resolution of native settings layers (§14.2, §14.4).
//!
//! A personal file is the user layer, its selected named profile is above
//! it, and --set values are nonpersistent session overrides. Keeping this
//! order independent of CLI argument position makes the same file portable
//! to a web storage adapter in M7.
use super::{DesktopSettings, file, json};
use puzzle_term::settings::{Layer, SettingsStore};
use std::path::PathBuf;

impl DesktopSettings {
    /// Loads a file, then profile and CLI overrides in precedence order.
    pub fn open(
        path: Option<&str>,
        profile: Option<&str>,
        sets: &[String],
    ) -> Result<(Self, Vec<String>), String> {
        // The path belongs to the desktop adapter. Neutral Session state
        // never reads HOME, XDG_CONFIG_HOME or APPDATA itself (R9).
        let path = path.map(PathBuf::from).unwrap_or_else(file::default_path);
        let mut settings = Self {
            store: SettingsStore::default(),
            path,
            modified: None,
            profile: profile.map(str::to_owned),
            zoom: 0,
        };
        // Resolve primary and the defaults before validating any config layer.
        settings
            .store
            .set_binding_platform(crate::keyboard::bindings::current(), false);
        let mut warnings = Vec::new();
        if let Err(error) = settings.reload() {
            // A malformed personal config must not prevent a shell from
            // opening. The warning is emitted after tracing initializes.
            warnings.push(error);
        }
        // An explicit --profile wins over a file's active profile. No
        // profile selection means the user layer remains above defaults.
        let selected = settings.profile.clone().or_else(|| {
            let value = &settings.store.effective().profile;
            (!value.is_empty()).then(|| value.clone())
        });
        if let Some(name) = selected {
            let report = settings.store.activate_profile(&name);
            if let Some(issue) = report.rejected.first() {
                // A missing named profile is reported while the user layer
                // stays valid; otherwise the shell would open at defaults.
                warnings.push(format!("{}: {}", issue.path, issue.message));
            }
        }
        for argument in sets {
            settings.apply_cli_set(argument)?;
        }
        Ok((settings, warnings))
    }

    /// Applies one transient key=value after parsing its JSON scalar form.
    fn apply_cli_set(&mut self, argument: &str) -> Result<(), String> {
        let (key, text) = argument
            .split_once('=')
            .ok_or_else(|| format!("--set needs key=value: {argument}"))?;
        if key.is_empty() {
            // An empty key could only become an unknown setting; treat it
            // as an option typo rather than silently ignoring user intent.
            return Err("--set needs a nonempty key".into());
        }
        let value = json::cli_value(text)?;
        let report = self.store.apply_key(key, value, Layer::Session);
        if let Some(issue) = report.rejected.first() {
            // A wrong-type command-line value is actionable immediately;
            // failing before window creation avoids a misleading launch.
            return Err(format!("--set {}: {}", issue.path, issue.message));
        }
        if !report.unknown.is_empty() {
            // Unknown keys in files survive for forward compatibility;
            // transient CLI typos have no such persistence reason.
            return Err(format!("--set unknown key: {key}"));
        }
        Ok(())
    }
}

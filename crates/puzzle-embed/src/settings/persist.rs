//! Native starter file, event-driven reload and user-layer save (§14.4).
//!
//! The typed store owns validation; this module only supplies JSON and a
//! path. A failed read, parse or field validation never replaces a prior
//! good configuration. Focus checks mtime once; an explicit binding forces
//! a retry even when the filesystem timestamp did not advance.
use super::{DesktopSettings, file, json};
use puzzle_term::settings::{Layer, SettingsStore, SettingsValue as Value};
use std::{collections::BTreeMap, path::PathBuf};

impl DesktopSettings {
    /// Writes a small starter document and its local editor schema once.
    pub fn init_config(path: Option<&str>) -> Result<PathBuf, String> {
        let path = path.map(PathBuf::from).unwrap_or_else(file::default_path);
        if path.exists() {
            // An explicit init must never overwrite a person's existing
            // settings, including future keys this build cannot interpret.
            return Err(format!("{} already exists", path.display()));
        }
        let schema_path = path.with_file_name("settings.schema.json");
        // Write the editor schema first: the starter's $schema URL should
        // resolve immediately when an editor opens the new config file.
        file::write(&schema_path, &json::schema::document())?;
        let mut settings = Self {
            store: SettingsStore::default(),
            path: path.clone(),
            modified: None,
            profile: None,
            zoom: 0,
        };
        // Only three examples keep the starter approachable; every other
        // key remains available through schema completion and defaults.
        for (key, value) in [
            ("$schema", Value::String("./settings.schema.json".into())),
            ("font.size", Value::Number(15.0)),
            ("theme.mode", Value::String("auto".into())),
        ] {
            let _ = settings.store.apply_key(key, value, Layer::User);
        }
        settings.save_user()?;
        Ok(path)
    }

    /// Reloads a changed file only after a focus event, never by polling.
    pub fn reload_if_changed(&mut self) -> Result<bool, String> {
        let latest = file::modified(&self.path)?;
        if latest == self.modified {
            // No parse or frame work belongs to an unchanged focus event.
            return Ok(false);
        }
        self.reload()?;
        Ok(true)
    }

    /// Reads and validates a whole user document before replacing its layer.
    pub fn reload(&mut self) -> Result<(), String> {
        self.modified = file::modified(&self.path)?;
        let Some(document) = file::read(&self.path)? else {
            // A missing file means defaults, including after the user
            // deliberately removes a formerly valid config.json.
            let _ = self.store.replace_user(&Value::Map(BTreeMap::new()));
            return Ok(());
        };
        // JSON parsing stays native. The same neutral Value validator
        // guards files, CLI values and future JS objects (§14.1).
        let value = json::from_json(&document, 0)?;
        let report = self.store.replace_user(&value);
        if let Some(issue) = report.rejected.first() {
            // replace_user prepared a clone; rejecting one leaf leaves the
            // previous complete user layer and session overrides intact.
            return Err(format!("{}: {}", issue.path, issue.message));
        }
        Ok(())
    }

    /// Saves only the user layer; CLI and profile overrides remain transient.
    pub fn save_user(&mut self) -> Result<(), String> {
        // Unknown JSON keys survive through the store's nested export,
        // even if an older Puzzle version cannot validate their meaning.
        let json = json::to_json(&self.store.export_user());
        file::write(&self.path, &json)?;
        self.modified = file::modified(&self.path)?;
        Ok(())
    }
}

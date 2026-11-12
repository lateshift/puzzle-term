//! Resolves validated defaults, user, profile and session layers (§14.2).
//!
//! A failed field leaves its prior value untouched. User-layer unknown keys
//! survive export, so an older build can round-trip a newer config file.
//! Recomputing typed effective fields after a write keeps layer precedence
//! independent of the order in which adapters deliver their patches.
//!
//! The maps store normalized values at dotted schema keys. This flat
//! internal form makes one setting easy to replace or reset without
//! disturbing its siblings. Export reconstructs the nested JSON shape
//! only at the persistence boundary (§14.4). The only maps that remain
//! opaque at this layer are `profiles` and `themes`: their user-defined
//! names are data, not schema path components.
//!
//! Validation happens before a value enters a layer. Recomputing the
//! effective typed struct can then apply user, profile and session in
//! precedence order without repeating validation for every read. A bad
//! file is prepared in a clone and committed only if every leaf passes;
//! a live terminal never loses its last good configuration (§14.4).
use super::{SettingsValue, schema::Settings};
use puzzle_bus::{Value, value::ValidationIssue};
use std::collections::BTreeMap;
/// Compatibility keys normalize before validation and layer storage.
mod aliases;
/// Binding validation and source-preserving platform resolution.
mod bindings;
/// Once-per-store reporting for the structural glyph budget floor.
mod budget;
/// Layer-specific merge and nested-document reconstruction.
mod merge;
use merge::{insert_nested, merge_bindings, merge_value};

/// Source layer for a validated settings patch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Layer {
    /// Persistent user configuration, including unrecognized future keys.
    User,
    /// Named overrides selected from the user document.
    Profile,
    /// Transient options for this terminal session.
    Session,
}

/// Per-key outcome visible to CLI, Rust and future JS adapters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApplyReport {
    /// Known keys accepted without a numeric change.
    pub applied: Vec<String>,
    /// Numeric keys accepted after bounded clamping.
    pub clamped: Vec<String>,
    /// Ill-typed or disallowed values, with their source paths.
    pub rejected: Vec<ValidationIssue>,
    /// Keys this build does not recognize but preserves in the user layer.
    pub unknown: Vec<String>,
    /// Parsed bindings whose feature or browser shortcut is unavailable.
    pub unavailable: Vec<ValidationIssue>,
    /// Accepted keys that require a new device or process session.
    pub next_session: Vec<String>,
}

/// Validated layers plus their current typed effective result.
#[derive(Clone, Debug)]
pub struct SettingsStore {
    /// Persistent settings from the platform's storage adapter.
    user: BTreeMap<String, Value>,
    /// Overrides for the active named profile, if any.
    profile: BTreeMap<String, Value>,
    /// Nonpersistent choices from CLI or a future JS call.
    session: BTreeMap<String, Value>,
    /// Active profile name used to resolve user-document overrides.
    active_profile: Option<String>,
    /// None keeps a platform-neutral schema-only store for headless scenarios.
    binding_platform: Option<crate::keybind::Platform>,
    /// Ordinary browsers warn about chrome-reserved shortcuts; hosts opt out.
    binding_web: bool,
    /// Typed values read by the hot rendering and terminal paths.
    effective: Settings,
    /// Malformed repeated reloads report the structural clamp only once.
    glyph_budget_warned: bool,
}

impl Default for SettingsStore {
    /// Starts from schema defaults without reading any platform file.
    fn default() -> Self {
        // Each platform owns its persistence adapter. Starting empty here
        // prevents this neutral crate from reading the user's home path.
        Self {
            user: BTreeMap::new(),
            profile: BTreeMap::new(),
            session: BTreeMap::new(),
            active_profile: None,
            effective: Settings::default(),
            binding_platform: None,
            binding_web: false,
            glyph_budget_warned: false,
        }
    }
}

impl SettingsStore {
    /// Borrows the typed result after all four layers have been resolved.
    ///
    /// Frame construction reads this snapshot rather than walking maps
    /// for every cell, keeping configuration lookup off the hot path.
    #[must_use]
    pub fn effective(&self) -> &Settings {
        &self.effective
    }

    /// Returns the active profile name without copying it.
    ///
    /// The profile map is already materialized, so callers need only this
    /// name for UI state and for an explicit persistence decision.
    #[must_use]
    pub fn active_profile(&self) -> Option<&str> {
        self.active_profile.as_deref()
    }

    /// Validates one dotted key and applies it at the selected layer.
    ///
    /// Invalid input never replaces a prior value. This matters for a
    /// partially edited config file and for future JS settings calls:
    /// both can arrive while a terminal is actively drawing.
    pub fn apply_key(&mut self, key: &str, value: SettingsValue, layer: Layer) -> ApplyReport {
        if let Some(report) = self.apply_mouse_alias(key, &value, layer) {
            return report;
        }
        let mut report = ApplyReport::default();
        if let Err(issues) = puzzle_bus::value::validate_tree(&value) {
            report.rejected.extend(issues.issues);
            return report;
        }
        let definition = Settings::schema().into_iter().find(|item| item.key == key);
        let Some(definition) = definition else {
            // Unknown user keys retain the exact plain-data value, including
            // nested maps, for a future version that understands the key.
            report.unknown.push(key.into());
            if layer == Layer::User {
                self.user.insert(key.into(), value);
            }
            return report;
        };
        let mut probe = Settings::default();
        // Schema assignment does type checks and numeric clamping once;
        // storing the normalized result makes later layer resolution
        // independent of the spelling of the original JSON number.
        // Individual binding failures skip only their entry; valid siblings
        // still apply, unlike a malformed JSON document (§14.9).
        let value = self.validate_bindings(key, value, &mut report);
        match probe.assign(key, value) {
            Ok(clamped) => {
                let normalized = probe.value(key).unwrap_or(Value::Null);
                self.layer_mut(layer).insert(key.into(), normalized);
                if clamped {
                    self.log_glyph_clamp(key);
                    report.clamped.push(key.into());
                } else {
                    report.applied.push(key.into());
                }
                if definition.apply == super::schema::ApplyMode::NextSession {
                    // Device creation and process-spawn options cannot be
                    // retroactively changed by a live settings patch.
                    report.next_session.push(key.into());
                }
                self.recompute();
            }
            Err(message) => {
                tracing::warn!(event = "settings.invalid", key, %message, "settings value rejected");
                report.rejected.push(ValidationIssue {
                    path: key.into(),
                    message,
                });
            }
        }
        report
    }

    /// Applies nested JSON-shaped maps, reporting every leaf independently.
    ///
    /// One rejected value must not mask valid siblings: a user may fix
    /// an individual field without losing other settings from the file.
    pub fn apply_patch(&mut self, patch: &Value, layer: Layer) -> ApplyReport {
        let mut report = ApplyReport::default();
        let Value::Map(map) = patch else {
            // The root must be an object so dotted keys have an
            // unambiguous parent in desktop JSON and web objects.
            report.rejected.push(ValidationIssue {
                path: "settings".into(),
                message: "expected an object".into(),
            });
            return report;
        };
        self.apply_map(map, "", layer, &mut report);
        report
    }

    /// Selects a named profile only after its stored override map validates.
    ///
    /// Profiles are selected atomically. A malformed profile leaves the
    /// previously active one visible, including its font and theme, until
    /// the user repairs the source document.
    pub fn activate_profile(&mut self, name: &str) -> ApplyReport {
        let mut report = ApplyReport::default();
        let candidate = self.user.get("profiles").and_then(|value| match value {
            Value::Map(map) => map.get(name),
            _ => None,
        });
        let Some(Value::Map(overrides)) = candidate else {
            // Rejecting an absent name is safer than silently falling
            // back to defaults while the UI still claims that profile.
            report.rejected.push(ValidationIssue {
                path: format!("profiles.{name}"),
                message: "profile does not exist or is not an object".into(),
            });
            return report;
        };
        // Build the replacement independently so a malformed profile cannot
        // partially displace the currently active, working configuration.
        let mut replacement = Self {
            binding_platform: self.binding_platform,
            binding_web: self.binding_web,
            glyph_budget_warned: self.glyph_budget_warned,
            ..Self::default()
        };
        replacement.apply_map(overrides, "", Layer::Profile, &mut report);
        self.glyph_budget_warned |= replacement.glyph_budget_warned;
        if report.has_fatal_rejection() {
            return report;
        }
        self.profile = replacement.profile;
        self.active_profile = Some(name.into());
        self.recompute();
        report
    }

    /// Exports exactly the persistent user layer, including unknown keys.
    ///
    /// Session and profile overrides are runtime choices. Exporting them
    /// would make a temporary CLI `--set` or selected profile permanent.
    #[must_use]
    pub fn export_user(&self) -> Value {
        let mut root = BTreeMap::new();
        // Rebuild nested objects only here; flat storage lets reset and
        // precedence address one schema key without editing sibling maps.
        for (key, value) in &self.user {
            insert_nested(&mut root, key, value.clone());
        }
        Value::Map(root)
    }

    /// Replaces a broken-file-free user document while preserving session edits.
    ///
    /// A reload may contain several values. Committing a partial file
    /// could change the terminal appearance before its parse error is
    /// shown, so validation is transactional for the entire user layer.
    pub fn replace_user(&mut self, patch: &Value) -> ApplyReport {
        let mut next = self.clone();
        // Session overrides live above this layer and survive the clone;
        // only the file-backed values are replaced on successful reload.
        next.user.clear();
        let mut report = next.apply_patch(patch, Layer::User);
        self.glyph_budget_warned |= next.glyph_budget_warned;
        if !report.has_fatal_rejection()
            && let Some(name) = next.active_profile.clone()
        {
            report.extend(next.activate_profile(&name));
        }
        if !report.has_fatal_rejection() {
            *self = next;
        }
        report
    }

    /// Removes one stored key and reveals the next lower layer.
    ///
    /// Removing an override does not copy defaults into its place: that
    /// would hide later edits to a lower-precedence source.
    pub fn reset(&mut self, key: &str, layer: Layer) {
        self.layer_mut(layer).remove(aliases::canonical(key));
        self.recompute();
    }

    /// Removes every value from one layer and reveals lower preferences.
    pub fn reset_all(&mut self, layer: Layer) {
        self.layer_mut(layer).clear();
        if layer == Layer::Profile {
            // A profile name without its overrides would mislead a UI.
            self.active_profile = None;
        }
        self.recompute();
    }

    /// Reports whether any source explicitly chooses a key over its default.
    #[must_use]
    pub fn has_override(&self, key: &str) -> bool {
        [&self.user, &self.profile, &self.session]
            .iter()
            .any(|layer| layer.contains_key(aliases::canonical(key)))
    }

    /// Chooses the mutable map without exposing a source's internals.
    ///
    /// All three maps share the same normalized key representation, so
    /// validation can be identical regardless of the caller's layer.
    fn layer_mut(&mut self, layer: Layer) -> &mut BTreeMap<String, Value> {
        match layer {
            Layer::User => &mut self.user,
            Layer::Profile => &mut self.profile,
            Layer::Session => &mut self.session,
        }
    }

    /// Visits each nested leaf, treating rich map fields as one schema key.
    ///
    /// Ordinary JSON objects encode dotted schema paths, while the keys
    /// below `profiles` and `themes` are user-chosen names. Descending
    /// into those names here would incorrectly report each as unknown.
    fn apply_map(
        &mut self,
        map: &BTreeMap<String, Value>,
        prefix: &str,
        layer: Layer,
        report: &mut ApplyReport,
    ) {
        for (name, value) in map {
            let key = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}.{name}")
            };
            let rich_map = matches!(key.as_str(), "profiles" | "themes");
            if let Value::Map(children) = value
                && !rich_map
            {
                self.apply_map(children, &key, layer, report);
            } else {
                report.extend(self.apply_key(&key, value.clone(), layer));
            }
        }
    }

    /// Resolves highest-layer wins, merging maps before typed assignment.
    ///
    /// Rendering uses this typed copy. Rebuilding after a patch is cold
    /// work; per-frame code never traverses a `Value` tree (§15 P1).
    fn recompute(&mut self) {
        let mut next = Settings::default();
        for definition in Settings::schema() {
            let mut value = definition.default;
            // Desktop's documented link default includes local files; explicit layers replace it.
            if definition.key == "links.schemes"
                && self.binding_platform.is_some()
                && !self.binding_web
                && let Value::List(schemes) = &mut value
            {
                schemes.push(Value::String("file".into()));
            }
            if definition.key == "keyboard.bindings" {
                value = self.default_bindings();
            }
            // Apply lower precedence first. A later scalar replaces it;
            // a later map changes only named children under that key.
            for layer in [&self.user, &self.profile, &self.session] {
                if let Some(override_value) = layer.get(definition.key) {
                    if definition.key == "keyboard.bindings" {
                        merge_bindings(
                            &mut value,
                            override_value,
                            self.binding_platform.unwrap_or_default(),
                        );
                    } else {
                        merge_value(&mut value, override_value.clone());
                    }
                }
            }
            // Each stored value passed the same assign check on arrival;
            // recomputing after a lower-layer edit cannot introduce failure.
            if let Err(message) = next.assign(definition.key, value) {
                // This indicates an internal mismatch: each stored leaf
                // passed the same schema when it entered its layer.
                tracing::warn!(event = "settings.recompute_failed", key=definition.key, %message);
            }
        }
        self.effective = next;
    }
}

impl ApplyReport {
    /// Appends one leaf's result while retaining source traversal order.
    ///
    /// Keeping all categories lets the UI show accepted fields next to
    /// rejected ones from the same submitted document.
    fn extend(&mut self, other: Self) {
        self.applied.extend(other.applied);
        self.clamped.extend(other.clamped);
        self.rejected.extend(other.rejected);
        self.unknown.extend(other.unknown);
        self.next_session.extend(other.next_session);
        self.unavailable.extend(other.unavailable);
    }
}

#[cfg(test)]
/// Layer, profile and hostile-value regressions.
mod tests;

#[cfg(test)]
/// Legacy Shift settings share canonical precedence and reset behavior.
mod shift_tests;

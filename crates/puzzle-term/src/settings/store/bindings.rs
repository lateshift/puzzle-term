//! Validates shortcuts per entry and retains settings-layer provenance.
//!
//! The effective string list remains portable export data. Introspection
//! rebuilds from original layers, because a flattened list cannot explain
//! which profile or session replaced a platform default (§14.9).
//!
//! A config can mix valid entries with misspelled actions or bad escapes.
//! Rejecting the entire bindings key would discard unrelated working keys,
//! so this validator records an indexed issue and keeps each valid sibling.
//! Future actions are warnings, not parse failures: a portable config may
//! refer to a feature a later milestone or another host can implement.
//! Browser-reserved shortcuts receive a separate availability report. The
//! same config can be usable in an embedded host without browser chrome.
//! Platform is supplied before loading so primary resolves to the user's OS.
//! An absent platform is useful for headless scenarios, which must not load
//! the machine's default shortcut table or user configuration implicitly.
//! The effective Settings string list remains exportable configuration data.
//! It cannot reconstruct source provenance after flattening, so introspection
//! walks the original layers in the same low-to-high order as recompute.
//! Default bindings are installed first unless clearDefaults resolves true.
//! A clear directive in a higher list removes even the installed defaults;
//! unbind removes just one canonical identity instead of clearing siblings.
//! Only binding-entry rejection is nonfatal to a document replacement.
//! Wrong types for ordinary settings still preserve last-good transactional
//! reload behavior; this exception cannot admit a broken font or theme value.
//! A non-string element is reported with its index, never converted to text.
//! That matters for JSON numbers and null, which are not shortcut syntax.
//! Profile and session layers retain their own sources even when they replace
//! a user trigger; the CLI and future settings editor can explain the winner.
//! The typed timeout is resolved through normal layer precedence, separate
//! from the special list merge, and zero remains the documented infinite wait.
//! Layer maps already contain only validated scalar settings. The explicit
//! string filter makes table rebuilding total even if future code introduces
//! another Value kind into a manually constructed settings document.
//! clearDefaults is resolved before traversing schema keys, because key
//! ordering must not accidentally change whether the initial table exists.
//! Deferred warnings retain the original entry's path while parser errors
//! carry their specific argument or grammar reason into ApplyReport.
use super::{ApplyReport, SettingsStore};
use crate::keybind::{Entry, Keybindings, Platform, Source, defaults};
use puzzle_bus::{Value, value::ValidationIssue};

/// Resolves portable bindings with their original layer ownership.
impl SettingsStore {
    /// Selects the user's OS defaults without core target-specific code.
    pub fn set_binding_platform(&mut self, platform: Platform, web: bool) {
        self.binding_platform = Some(platform);
        self.binding_web = web;
        self.recompute();
    }
    /// Resolves the shared matcher with a source label for every entry.
    pub fn keybindings(&self) -> Keybindings {
        let platform = self.binding_platform.unwrap_or_default();
        let mut bindings = Keybindings::default();
        bindings.timeout_ms = u64::from(self.effective.keyboard_sequence_timeout_ms);
        if self.binding_platform.is_some() && !self.effective.keyboard_clear_defaults {
            bindings.apply(
                &defaults::lines(platform),
                Source::Defaults,
                platform,
                self.binding_web,
            );
        }
        // A profile can override one trigger while preserving unrelated
        // lower-layer entries. Session options are always strongest.
        for (layer, source) in [
            (&self.user, Source::User),
            (&self.profile, Source::Profile),
            (&self.session, Source::Session),
        ] {
            if let Some(Value::List(lines)) = layer.get("keyboard.bindings") {
                let lines = lines
                    .iter()
                    .filter_map(|v| {
                        if let Value::String(s) = v {
                            Some(s.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                bindings.apply(&lines, source, platform, self.binding_web);
            }
        }
        bindings
    }
    /// Removes invalid entries before schema assignment, reporting their index.
    pub(super) fn validate_bindings(
        &self,
        key: &str,
        value: Value,
        report: &mut ApplyReport,
    ) -> Value {
        if key != "keyboard.bindings" {
            return value;
        }
        let Value::List(lines) = value else {
            return value;
        };
        let mut valid = Vec::new();
        for (index, line) in lines.into_iter().enumerate() {
            let path = format!("keyboard.bindings[{index}]");
            let Value::String(text) = &line else {
                report.rejected.push(ValidationIssue {
                    path,
                    message: "binding must be a string".into(),
                });
                continue;
            };
            match Entry::parse(
                text,
                self.binding_platform.unwrap_or_default(),
                Source::User,
            ) {
                Ok(Entry::Clear) => valid.push(line),
                Ok(Entry::Binding(binding)) => {
                    // Unavailable is a warning, not a parse error: preserving
                    // future actions allows sharing files across releases.
                    let when = binding.action.info().available;
                    if when != "now" {
                        report.unavailable.push(ValidationIssue {
                            path: path.clone(),
                            message: format!("available from {when}"),
                        });
                    }
                    if self.binding_web
                        && binding.reserved(self.binding_platform.unwrap_or_default())
                    {
                        report.unavailable.push(ValidationIssue {
                            path,
                            message: "browser-reserved shortcut".into(),
                        });
                    }
                    valid.push(line);
                }
                Err(message) => report.rejected.push(ValidationIssue { path, message }),
            }
        }
        Value::List(valid)
    }
    /// Platform defaults are the bottom layer unless explicitly cleared.
    pub(super) fn default_bindings(&self) -> Value {
        // clearDefaults may be overridden by any source. Resolve this one
        // boolean before traversing the bindings key, not by schema order.
        let clear = [&self.session, &self.profile, &self.user]
            .iter()
            .find_map(|layer| layer.get("keyboard.clearDefaults"))
            .is_some_and(|value| *value == Value::Bool(true));
        self.binding_platform
            .filter(|_| !clear)
            .map_or(Value::List(Vec::new()), |platform| {
                Value::List(
                    defaults::lines(platform)
                        .into_iter()
                        .map(Value::String)
                        .collect(),
                )
            })
    }
}

/// Distinguishes a skipped shortcut from a failed document transaction.
impl ApplyReport {
    /// A bad binding is skipped; other rejected settings retain transactionality.
    pub(super) fn has_fatal_rejection(&self) -> bool {
        self.rejected
            .iter()
            .any(|issue| !issue.path.starts_with("keyboard.bindings["))
    }
}

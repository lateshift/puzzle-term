//! Structured filter parsing and deterministic test capture (design §5.6).
//!
//! `Targets` accepts module-level directives without the regex-heavy env
//! filter. Shells supply a default level and an optional user string; this
//! module validates that string once during startup. Tests use `MemoryLayer`
//! to assert stable diagnostic event fields instead of brittle stderr text.
use std::{
    collections::BTreeMap,
    str::FromStr,
    sync::{Arc, Mutex},
};
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{Layer, filter::Targets, layer::Context};

/// Parses `warn,puzzle_vt=debug` style target directives.
///
/// The caller chooses the default (`warn` web, `info` desktop). Malformed
/// input is reported at startup rather than silently altering log volume.
pub fn parse_filter(spec: &str) -> Result<Targets, String> {
    // An empty directive is usually a missing option value, so surface it
    // instead of accidentally replacing the platform's default filter.
    if spec.trim().is_empty() {
        return Err("log filter is empty".into());
    }
    Targets::from_str(spec).map_err(|error| error.to_string())
}

/// One captured tracing event with its stable target and structured fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogRecord {
    /// Severity as displayed by tracing.
    pub level: tracing::Level,
    /// Module target, used by target filters.
    pub target: String,
    /// Field names and formatted values, including the stable `event` key.
    pub fields: BTreeMap<String, String>,
}

/// Shared capture sink for deterministic diagnostics assertions.
#[derive(Clone, Default)]
pub struct MemoryLayer {
    /// A shared sink is needed because the subscriber owns a cloned layer.
    records: Arc<Mutex<Vec<LogRecord>>>,
}

/// Reading captured records does not depend on global tracing state.
impl MemoryLayer {
    /// Returns a snapshot even when another thread still owns the subscriber.
    #[must_use]
    pub fn records(&self) -> Vec<LogRecord> {
        // The lock may be poisoned by a failing test; preserve the earlier
        // diagnostics rather than causing a second panic during assertion.
        self.records
            .lock()
            .map(|records| records.clone())
            .unwrap_or_default()
    }
}

/// Field visitor that preserves names for stable event assertions.
struct FieldVisitor {
    /// Captured values are already formatted because tracing fields vary by type.
    values: BTreeMap<String, String>,
}

/// Collects the primitive and debug field representations supplied by tracing.
impl Visit for FieldVisitor {
    /// Debug values include structured diagnostic payloads such as hook chains.
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.values
            .insert(field.name().to_owned(), format!("{value:?}"));
    }
}

/// Captures events under the subscriber selected by the test.
impl<S: Subscriber> Layer<S> for MemoryLayer {
    /// Writes one record without printing or installing a global collector.
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor {
            values: BTreeMap::new(),
        };
        event.record(&mut visitor);
        let metadata = event.metadata();
        let record = LogRecord {
            level: *metadata.level(),
            target: metadata.target().to_owned(),
            fields: visitor.values,
        };
        if let Ok(mut records) = self.records.lock() {
            records.push(record);
        }
    }
}

#[cfg(test)]
/// Filter parsing and stable captured diagnostics.
mod tests;

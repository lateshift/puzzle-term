//! The generic extension registry validates names and typed payloads.
// Test assertions may panic to identify a broken fixture during development.
#![allow(
    clippy::expect_used,
    reason = "test assertions identify invalid fixtures"
)]
use super::{Extension, ExtensionRegistry, ToWire, WireExt};
use crate::value::{ValidationReport, Value};

/// A typed plugin value used without any terminal message enum.
#[derive(Debug, PartialEq)]
struct PluginCount(u32);

/// Plugin conversion rejects fractional or negative public counts.
impl WireExt for PluginCount {
    /// Namespace and kind are stable protocol identity.
    const TYPE_NAME: &'static str = "ext:sample/count";
    /// A count travels as an exact finite number.
    fn to_value(&self) -> Value {
        Value::Number(f64::from(self.0))
    }
    /// Only nonnegative whole values fit the typed payload.
    fn from_value(value: &Value) -> Result<Self, ValidationReport> {
        if let Value::Number(count) = value
            && count.is_finite()
            && *count >= 0.0
            && *count <= f64::from(u32::MAX)
            && count.fract() == 0.0
        {
            return Ok(Self(*count as u32));
        }
        Err(super::reject("expected unsigned count"))
    }
}

/// Registry decoding preserves both the Rust type and its wire snapshot.
#[test]
fn plugin_registry_round_trips_without_domain_types() {
    let mut registry = ExtensionRegistry::default();
    registry
        .register::<PluginCount>()
        .expect("valid plugin name");
    let payload = Value::Number(7.0);
    let decoded = registry
        .decode(PluginCount::TYPE_NAME, &payload)
        .expect("valid payload");
    assert_eq!(decoded.downcast_ref::<PluginCount>(), Some(&PluginCount(7)));
    assert_eq!(decoded.to_value().expect("wire snapshot"), payload);
    let built = Extension::new(PluginCount(7)).expect("typed plugin");
    assert_eq!(built.type_name(), PluginCount::TYPE_NAME);
}

/// A registered plugin never turns arbitrary numbers into a typed count.
#[test]
fn bad_extension_payload_returns_report() {
    let mut registry = ExtensionRegistry::default();
    registry
        .register::<PluginCount>()
        .expect("valid plugin name");
    for payload in [Value::Number(-1.0), Value::Number(1.5), Value::Bool(true)] {
        assert!(registry.decode(PluginCount::TYPE_NAME, &payload).is_err());
    }
}

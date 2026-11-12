//! Fallible plain-data boundary shared by domain-owned messages (design §5.7).
//!
//! `Value` is a transport representation, not a substitute for a Rust
//! payload type. Domain enums implement these traits beside their concrete
//! variants. The generic bus calls `FromWire` before it attaches `Origin::Api`.
/// Registered typed plugin messages and their one `Any` seam.
mod extension;

use crate::{
    kinds::Shape,
    value::{ValidationReport, Value},
};
pub use extension::{Extension, ExtensionRegistry, WireExt};

/// Converts a typed message to its stable name and plain payload.
pub trait ToWire {
    /// Stable protocol spelling, including a registered extension name.
    fn type_name(&self) -> &str;
    /// Builds a bounded external value or reports why it is unavailable.
    fn to_value(&self) -> Result<Value, ValidationReport>;
}

/// Validates external plain data before constructing a typed enum.
pub trait FromWire: Sized {
    /// Reads one type string and value using registered plugin decoders.
    fn from_value(
        type_name: &str,
        value: &Value,
        extensions: &ExtensionRegistry,
    ) -> Result<Self, ValidationReport>;
}

/// Checks the outer kind shape before a domain inspects named fields.
pub fn check_shape(shape: Shape, value: &Value) -> Result<(), ValidationReport> {
    // The outer shape is cheap to test and gives the same error location
    // for every domain before its field-level decoder examines the value.
    let valid = match (shape, value) {
        // A registered extension performs its own payload validation; the
        // bounded tree walk still runs before its decoder is invoked.
        (Shape::Any, _) => true,
        (Shape::Null, Value::Null)
        | (Shape::String, Value::String(_))
        | (Shape::Bool, Value::Bool(_))
        | (Shape::Map, Value::Map(_))
        | (Shape::Bytes, Value::Bytes(_)) => true,
        // NaN and infinities cannot round-trip through JSON, so reject them
        // even when a Rust caller constructs a Number directly.
        (Shape::Number, Value::Number(number)) => number.is_finite(),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        // Reports are data, not panics: browser arguments and recordings are
        // untrusted inputs at this boundary (R8).
        let mut report = ValidationReport::default();
        report.push("payload", format!("expected {shape:?}"));
        Err(report)
    }
}

/// Creates one location-specific report for an invalid protocol identity.
#[must_use]
pub fn reject(message: &str) -> ValidationReport {
    let mut report = ValidationReport::default();
    report.push("type", message);
    report
}

#[cfg(test)]
/// Plugin registry tests independent of terminal messages.
mod tests;

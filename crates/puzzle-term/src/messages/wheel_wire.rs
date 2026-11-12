//! Unit-tagged wheel payloads keep pixel inertia distinct from row notches.
//!
//! The pointer subrecord reuses the existing exact integer validation. Units
//! are explicit; a replay never guesses a device type from delta magnitude.
use super::{Command, Value, mouse_wire, viewport_wire};
use crate::viewport::ScrollInput;
use puzzle_bus::{value::ValidationReport, wire::reject};
use puzzle_vt::input::MouseEvent;
use std::collections::BTreeMap;
/// Allocates plain data only for recording or external subscribers.
pub(super) fn to_value(delta: ScrollInput, pointer: &MouseEvent) -> Value {
    // A unit tag survives recordings across devices with different cell heights.
    let (unit, amount) = match delta {
        ScrollInput::Pixels(value) => ("pixels", value),
        ScrollInput::Rows(value) => ("rows", value),
    };
    // The three fields are independent: magnitude, interpretation and pointer hit.
    Value::Map(BTreeMap::from([
        ("unit".into(), Value::String(unit.into())),
        // f64 preserves fractional device deltas through the neutral value tree.
        ("delta".into(), Value::Number(amount)),
        ("pointer".into(), mouse_wire::to_value(pointer)),
    ]))
}
/// Rejects unknown units, nonfinite deltas and misshapen nested hits.
pub(super) fn from_value(value: &Value) -> Result<Command, ValidationReport> {
    // An outer map is required even when the nested pointer is otherwise valid.
    let Value::Map(map) = value else {
        return Err(reject("wheel must be a map"));
    };
    // Exact arity prevents unrecognized fields or forged origin metadata.
    if map.len() != 3 {
        return Err(reject("wheel needs unit, delta and pointer"));
    }
    // Check finite f64 before either branch can reach clamp or animation math.
    let delta = viewport_wire::number(
        map.get("delta")
            .ok_or_else(|| reject("missing wheel delta"))?,
    )?;
    // Unknown strings never fall back to row units and reinterpret movement.
    let delta = match map.get("unit") {
        Some(Value::String(unit)) if unit == "pixels" => ScrollInput::Pixels(delta),
        Some(Value::String(unit)) if unit == "rows" => ScrollInput::Rows(delta),
        _ => return Err(reject("wheel unit must be pixels or rows")),
    };
    // Reuse bounded coordinate/modifier checks; pixel and cell positions differ.
    // Nested fields are validated before a typed wheel can enter interceptors.
    let pointer = mouse_wire::from_value(
        map.get("pointer")
            .ok_or_else(|| reject("missing wheel pointer"))?,
    )?;
    // Origin belongs to the caller/bus; it cannot be set by this payload.
    Ok(Command::Wheel { delta, pointer })
}

//! Exact pointer maps preserve signed outside-grid distance for selection (§5.7).
//!
//! Cell and protocol coordinates are copied at the host edge. External payloads
//! reject nonfinite positions and zero geometry before reaching hit arithmetic.
//!
//! The nested event keeps the existing Mouse payload's zero-based cell and pixel
//! convention. The x/y fields are different: they retain negative coordinates
//! above or left of the grid and positions beyond its bottom or right edge.
//! Clamping those fields here would erase the evidence used for auto-scroll.
//!
//! Device-cell dimensions travel with the sample for deterministic replay when
//! no text context is installed. A rendered Session uses its active builder's
//! geometry instead, so a stale host callback cannot reinterpret a resized grid.
//! The exact map cardinality plus required-field checks reject extra keys.
//! Geometry is integral and nonzero; coordinates may legitimately be fractional.
//! Every numeric value must be finite before floor, clamp or speed calculations.
//! The decoder never creates registry handles or changes a viewport itself.
//! Origins remain bus metadata and cannot be supplied by the pointer payload.

use crate::pointer::PointerInput;
use puzzle_bus::{Value, value::ValidationReport, wire::reject};
use std::collections::BTreeMap;

/// Converts copied input data without retaining any host or terminal borrow.
pub(super) fn to_value(input: PointerInput) -> Value {
    Value::Map(BTreeMap::from([
        ("event".into(), super::mouse_wire::to_value(&input.event)),
        ("x".into(), Value::Number(input.position[0])),
        ("y".into(), Value::Number(input.position[1])),
        ("cellWidth".into(), Value::Number(f64::from(input.cell[0]))),
        ("cellHeight".into(), Value::Number(f64::from(input.cell[1]))),
    ]))
}

/// Requires precisely one event, two coordinates and two positive dimensions.
pub(super) fn from_value(value: &Value) -> Result<PointerInput, ValidationReport> {
    let Value::Map(map) = value else {
        return Err(reject("pointer requires a map"));
    };
    if map.len() != 5 {
        return Err(reject(
            "pointer requires event, x, y, cellWidth and cellHeight",
        ));
    }
    let event = super::mouse_wire::from_value(
        map.get("event")
            .ok_or_else(|| reject("pointer event missing"))?,
    )?;
    let x = number(map, "x")?;
    let y = number(map, "y")?;
    let width = number(map, "cellWidth")?;
    let height = number(map, "cellHeight")?;
    // Exact integer geometry avoids silent unit changes in recordings and API input.
    if [width, height]
        .iter()
        .any(|n| *n < 1.0 || *n > f64::from(u32::MAX) || n.fract() != 0.0)
    {
        return Err(reject(
            "pointer cell geometry must be positive u32 integers",
        ));
    }
    Ok(PointerInput {
        event,
        position: [x, y],
        cell: [width as u32, height as u32],
    })
}

/// Finite coordinates keep clamping, row conversion and speed integration defined.
fn number(map: &BTreeMap<String, Value>, key: &str) -> Result<f64, ValidationReport> {
    match map.get(key) {
        Some(Value::Number(n)) if n.is_finite() => Ok(*n),
        _ => Err(reject("pointer numeric field must be finite")),
    }
}

//! Bounded pointer payloads at the public input/mouse boundary (§5.7).
//!
//! Device pixels and cell coordinates are retained independently; replay
//! must not recompute a recorded hit using today's font metrics. All integer
//! fields are validated before narrowing, including the future selection
//! click count. Trusted Origin is attached by the bus, never in this map.
//! A press retains the actual button identity in every encoding. The encoder
//! alone converts legacy releases to button 3, so SGR release can still use
//! the original button with a lowercase final m.
//! Coordinates are zero-based here; the wire to the terminal application is
//! one-based. Adding that offset during recording would apply it twice.
//! Button-held and click-count describe different facts: a second selection
//! click may occur after every physical button was released.
//! A count of zero is meaningful on hover and wheel records. Counts above
//! three are rejected because local selection exposes only single/double/triple.
//! Field names are stable camelCase, matching the existing public key payload.
//! Typed callbacks never allocate this map; conversion occurs only when a
//! recorder or external subscriber requests a plain-data representation.
use puzzle_bus::{Value, value::ValidationReport, wire::reject};
use puzzle_vt::input::{Modifiers, MouseEvent, MouseKind};
use std::collections::BTreeMap;

/// Emits a complete, unambiguous pointer record only for wire consumers.
pub(super) fn to_value(event: &MouseEvent) -> Value {
    let kind = match event.kind {
        MouseKind::Press => "press",
        MouseKind::Release => "release",
        MouseKind::Move => "move",
        MouseKind::Wheel => "wheel",
    };
    let mut map = BTreeMap::from([
        ("kind".into(), Value::String(kind.into())),
        ("held".into(), Value::Bool(event.button_held)),
    ]);
    // Numeric values are exact in f64 because each source fits u32.
    for (key, number) in [
        ("button", u32::from(event.button)),
        ("column", event.column),
        ("row", event.row),
        ("pixelX", event.pixel_x),
        ("pixelY", event.pixel_y),
        ("modifiers", u32::from(event.modifiers.0)),
        ("clickCount", u32::from(event.click_count)),
    ] {
        map.insert(key.into(), Value::Number(f64::from(number)));
    }
    Value::Map(map)
}

/// Rejects missing, extra, fractional and overflowing external fields.
pub(super) fn from_value(value: &Value) -> Result<MouseEvent, ValidationReport> {
    let Value::Map(map) = value else {
        return Err(reject("mouse payload must be a map"));
    };
    // Exactly nine keys also prevents callers from forging origin metadata.
    if map.len() != 9 {
        return Err(reject("mouse payload needs nine fields"));
    }
    // Interpret transition strings before constructing a typed event;
    // unknown words cannot fall through as a mouse press.
    let kind = match map.get("kind") {
        Some(Value::String(name)) => match name.as_str() {
            "press" => MouseKind::Press,
            "release" => MouseKind::Release,
            "move" => MouseKind::Move,
            "wheel" => MouseKind::Wheel,
            _ => return Err(reject("unknown mouse transition")),
        },
        _ => return Err(reject("mouse kind must be text")),
    };
    let Some(Value::Bool(held)) = map.get("held") else {
        return Err(reject("mouse held must be boolean"));
    };
    // Each integer helper checks finiteness, fraction and range before cast.
    // This also rejects a missing required key even when map length is nine.
    Ok(MouseEvent {
        kind,
        // Reject overflow instead of silently changing a vendor button code.
        button: integer(map, "button", u32::from(u16::MAX))? as u16,
        column: integer(map, "column", u32::MAX)?,
        row: integer(map, "row", u32::MAX)?,
        pixel_x: integer(map, "pixelX", u32::MAX)?,
        pixel_y: integer(map, "pixelY", u32::MAX)?,
        // Only the four public modifier bits may enter the typed encoder.
        modifiers: Modifiers(integer(map, "modifiers", 15)? as u8),
        button_held: *held,
        click_count: integer(map, "clickCount", 3)? as u8,
    })
}

/// Checks representation before a cast so hostile numbers cannot saturate.
fn integer(map: &BTreeMap<String, Value>, key: &str, max: u32) -> Result<u32, ValidationReport> {
    if let Some(Value::Number(number)) = map.get(key)
        && number.is_finite()
        && *number >= 0.0
        && *number <= f64::from(max)
        && number.fract() == 0.0
    {
        return Ok(*number as u32);
    }
    Err(reject(&format!(
        "mouse {key} must be an integer in 0..={max}"
    )))
}

//! Bounded IME wire data without interpreting provisional text as input.
//!
//! A caret is a UTF-8 boundary in this string. DOM UTF-16 offsets must be
//! converted by the adapter; neither replay nor overlays may slice a scalar.
//! Null means the host supplied no caret; zero is a real start-of-string
//! caret and must not be collapsed to absence. Outer tree validation applies
//! the same string and nesting limits as every public message. Exact keys
//! reject accidental replay metadata instead of silently ignoring a typo.
//! Both encoders validate so native constructors cannot publish a wire value
//! that the external decoder would reject on its next round trip.
use super::PreeditState;
use puzzle_bus::{
    value::{ValidationReport, Value},
    wire::reject,
};
use std::collections::BTreeMap;

/// Encodes a real composition, rejecting an invalid host-supplied caret.
pub(super) fn to_value(state: &PreeditState) -> Result<Value, ValidationReport> {
    // Validate both directions: native hosts can construct these structs too.
    if state
        .cursor
        .is_some_and(|offset| !state.text.is_char_boundary(offset))
    {
        return Err(reject("preedit cursor must be a UTF-8 boundary"));
    }
    Ok(Value::Map(BTreeMap::from([
        ("text".into(), Value::String(state.text.clone())),
        (
            // Preserve absence as null. Omitting the caret key would make a
            // generated recording ambiguous between unknown and malformed.
            "cursor".into(),
            state
                .cursor
                .map_or(Value::Null, |n| Value::Number(n as f64)),
        ),
    ])))
}

/// Rejects extra fields, fractional offsets and offsets inside multibyte text.
pub(super) fn from_value(value: &Value) -> Result<PreeditState, ValidationReport> {
    let Value::Map(map) = value else {
        return Err(reject("preedit must be a map"));
    };
    let Some(Value::String(text)) = map.get("text") else {
        return Err(reject("preedit text must be a string"));
    };
    if map.len() != 2 {
        // External callers cannot attach trusted origin or replay fields here.
        return Err(reject("unexpected preedit fields"));
    }
    // Check against string length before narrowing an external number.
    let cursor = match map.get("cursor") {
        Some(Value::Null) => None,
        Some(Value::Number(n))
            // Infinity and fractional values must fail before the cast can
            // saturate them into a plausible integer caret position.
            if n.is_finite() && *n >= 0.0 && *n <= text.len() as f64 && n.fract() == 0.0 =>
        {
            Some(*n as usize)
        }
        _ => return Err(reject("invalid preedit cursor")),
    };
    let state = PreeditState {
        text: text.clone(),
        cursor,
    };
    // Reuse the boundary invariant rather than introducing a second rule.
    to_value(&state)?;
    Ok(state)
}

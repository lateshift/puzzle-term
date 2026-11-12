//! Typed viewport payload validation at the external bus boundary (§5.7).
//!
//! Distances must be finite. IDs use JS's exact-integer range, and navigation
//! maps reject unknown fields instead of silently altering recorded intent.
//!
//! The external convention is rows for relative commands and device pixels
//! for scrolled facts. Conversion never applies a font scale: the viewport
//! owner alone knows the active device-cell height. This avoids recordings
//! changing meaning when a terminal moves between different-DPR displays.
//!
//! Absolute targets need no current buffer borrow. A retained LineId may
//! disappear between decoding and dispatch; the handler then leaves the
//! viewport unchanged, rather than treating the numeric identity as an index.
//! Prompt and match navigation instead carry signed document-order counts.
//! A direction of zero is a valid no-op and stays exactly zero on replay.
//!
//! JavaScript numbers represent integers exactly only through 2^53 - 1.
//! The ID validator enforces that range before narrowing to Rust's u64.
//! A fractional identity would silently name another line after a cast.
//! Direction counts use i32 because all keyboard and bus navigation share
//! that type; wider numbers are rejected instead of wrapping at dispatch.
//!
//! Top and bottom maps contain only their discriminator. Line and relative
//! marker targets require exactly one additional field. Both a misspelled
//! field and a surplus field fail, exposing recording/schema mistakes early.
use crate::viewport::ScrollTarget;
use puzzle_bus::{
    value::{ValidationReport, Value},
    wire::reject,
};
use std::collections::BTreeMap;
/// Rejects NaN and infinities before they enter the device-pixel model.
pub(super) fn number(value: &Value) -> Result<f64, ValidationReport> {
    match value {
        Value::Number(value) if value.is_finite() => Ok(*value),
        _ => Err(reject("scroll distance must be finite")),
    }
}
/// Encodes one absolute navigation request with a single discriminator.
pub(super) fn target_value(target: ScrollTarget) -> Value {
    // Maps are allocated only at this external subscription/recording seam.
    let mut map = BTreeMap::new();
    let name = match target {
        ScrollTarget::Top => "top",
        ScrollTarget::Bottom => "bottom",
        ScrollTarget::Line(line) | ScrollTarget::LineTop(line) => {
            map.insert("lineId".into(), Value::Number(line.0 as f64));
            if matches!(target, ScrollTarget::LineTop(_)) {
                "lineTop"
            } else {
                "line"
            }
        }
        ScrollTarget::Prompt(delta) => {
            map.insert("delta".into(), Value::Number(f64::from(delta)));
            "prompt"
        }
        ScrollTarget::Match(delta) => {
            map.insert("delta".into(), Value::Number(f64::from(delta)));
            "match"
        }
    };
    map.insert("target".into(), Value::String(name.into()));
    Value::Map(map)
}
/// Decodes the closed target enum and rejects fractional IDs/directions.
pub(super) fn parse_target(value: &Value) -> Result<ScrollTarget, ValidationReport> {
    let Value::Map(map) = value else {
        return Err(reject("scroll target must be a map"));
    };
    let Some(Value::String(name)) = map.get("target") else {
        return Err(reject("scroll target name missing"));
    };
    // Decode by the closed enum name after proving the payload is a map.
    match name.as_str() {
        "top" | "bottom" if map.len() == 1 => Ok(if name == "top" {
            ScrollTarget::Top
        } else {
            ScrollTarget::Bottom
        }),
        "line" | "lineTop" if map.len() == 2 => {
            let id = number(map.get("lineId").ok_or_else(|| reject("lineId missing"))?)?;
            if id.fract() != 0.0 || !(0.0..=9_007_199_254_740_991.0).contains(&id) {
                return Err(reject("lineId must be an exact nonnegative integer"));
            }
            Ok(if name == "lineTop" {
                ScrollTarget::LineTop(puzzle_vt::tracked::LineId(id as u64))
            } else {
                ScrollTarget::Line(puzzle_vt::tracked::LineId(id as u64))
            })
        }
        "prompt" | "match" if map.len() == 2 => {
            let delta = number(map.get("delta").ok_or_else(|| reject("delta missing"))?)?;
            if delta.fract() != 0.0 || delta < f64::from(i32::MIN) || delta > f64::from(i32::MAX) {
                return Err(reject("navigation delta must be an i32"));
            }
            Ok(if name == "prompt" {
                ScrollTarget::Prompt(delta as i32)
            } else {
                ScrollTarget::Match(delta as i32)
            })
        }
        _ => Err(reject("unknown target or unexpected navigation fields")),
    }
}

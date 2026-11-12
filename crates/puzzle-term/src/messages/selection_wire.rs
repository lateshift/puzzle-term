//! Bounded selection positions at the public bus boundary (§5.7).
//!
//! Range endpoints are inclusive cells; Between carries half-open text gaps.
//! JS-exact IDs and finite columns are checked before narrowing. A missing
//! retained row is handled later at dispatch, because output can evict it
//! between validation and execution. Unknown fields never alter intent.
//!
//! Range requires kind/start/end; Between requires only start/end text gaps.
//! All and clear carry only their operation discriminator. Command-output
//! selection has a nullable at position: null means the latest shell output.
//! These shapes make recording replay independent of the current viewport.
//!
//! Line IDs are not physical row numbers. Their upper bound is the largest
//! integer JavaScript can represent exactly. Columns are nonnegative u32
//! integers at this boundary; actual grid membership is validated by VT later.
//! No unchecked cast runs before the finite/integer/range checks succeed.
//! Origin is assigned by the bus dispatch seam and is absent from this map.
//!
//! SelectionChanged carries one nullable range. Its block flag determines
//! whether first/last columns describe a rectangle or a linear interval.
//! It contains no copied text: a subscriber asks the contents API explicitly
//! when it needs a string, avoiding text duplication on every drag update.
//! Encoding happens only for an actual external subscription or recording.
//! Normal internal hooks receive SelectionCommand and SelectionRange directly.
//!
//! A stale retained identity is not a malformed wire value. Output may evict
//! it after decoding; the Session leaves the current selection unchanged in
//! that case. Structural errors remain ValidationReports and never enqueue.

use crate::{
    selection::{SelectionCommand, SelectionRange},
    vt::{
        selection::{SelectPoint, SelectionKind},
        tracked::LineId,
    },
};
use puzzle_bus::{Value, value::ValidationReport, wire::reject};
use std::collections::BTreeMap;
/// Encodes one cell position using the public lineId/col convention.
fn point_value(point: SelectPoint) -> Value {
    // A position contains identity and column, with no viewport-dependent row number.
    Value::Map(BTreeMap::from([
        ("lineId".into(), Value::Number(point.line.0 as f64)),
        ("col".into(), Value::Number(point.column as f64)),
    ]))
}
/// Rejects fractional, nonfinite and inexact identities and columns.
fn point(value: &Value) -> Result<SelectPoint, ValidationReport> {
    // Reject scalar/list payloads before any map field lookup or numeric cast.
    let Value::Map(map) = value else {
        return Err(reject("position must be a map"));
    };
    // Exact cardinality rejects hidden fields rather than silently changing the schema.
    if map.len() != 2 {
        return Err(reject("position requires lineId and col"));
    }
    // Numbers cross the JS seam as f64, so integral/range checks precede narrowing.
    // The caller supplies the distinct LineId and column limits.
    let integer = |key: &str, max: f64| -> Result<f64, ValidationReport> {
        let Some(Value::Number(n)) = map.get(key) else {
            return Err(reject("position field must be numeric"));
        };
        // NaN and infinity evade ordinary comparisons; exclude them explicitly.
        // Fractions must never be truncated into a different selected cell.
        if !n.is_finite() || n.fract() != 0.0 || !(0.0..=max).contains(n) {
            return Err(reject(
                "position field must be an exact nonnegative integer",
            ));
        }
        Ok(*n)
    };
    // Successful checks establish safe exact integer conversions on both targets.
    Ok(SelectPoint {
        line: LineId(integer("lineId", 9_007_199_254_740_991.0)? as u64),
        column: integer("col", f64::from(u32::MAX))? as usize,
    })
}
/// Converts the closed gesture vocabulary to a stable wire spelling.
fn kind_name(kind: SelectionKind) -> &'static str {
    // These strings are public protocol vocabulary, independent of Rust debug names.
    match kind {
        SelectionKind::Simple => "simple",
        SelectionKind::Word => "word",
        SelectionKind::Line => "line",
        SelectionKind::Block => "block",
        SelectionKind::Semantic => "semantic",
    }
}
/// Serializes only when an external observer or recording requests data.
pub(super) fn to_value(command: SelectionCommand) -> Value {
    // Encoding is a cold external-observation path, so owned maps are intentional.
    let mut map = BTreeMap::new();
    let operation = match command {
        SelectionCommand::Between { start, end } => {
            map.insert("start".into(), point_value(start));
            map.insert("end".into(), point_value(end));
            "between"
        }
        // Range records capture the gesture’s original expansion kind and endpoints.
        SelectionCommand::Range { kind, start, end } => {
            map.insert("kind".into(), Value::String(kind_name(kind).into()));
            map.insert("start".into(), point_value(start));
            map.insert("end".into(), point_value(end));
            "range"
        }
        // Null requests the latest output; a point records semantic pointer intent.
        SelectionCommand::CommandOutput(at) => {
            map.insert("at".into(), at.map_or(Value::Null, point_value));
            "commandOutput"
        }
        SelectionCommand::All => "all",
        SelectionCommand::Clear => "clear",
    };
    // One discriminator makes all shapes unambiguous during replay.
    map.insert("operation".into(), Value::String(operation.into()));
    Value::Map(map)
}
/// Decodes exactly one typed request, rejecting extra/missing fields.
pub(super) fn from_value(value: &Value) -> Result<SelectionCommand, ValidationReport> {
    // Reject scalar/list payloads before any map field lookup or numeric cast.
    let Value::Map(map) = value else {
        return Err(reject("selection must be a map"));
    };
    // Cardinality plus named required fields rejects extra and missing keys together.
    // No terminal lookup belongs here: a valid ID can become evicted before pump.
    match map.get("operation") {
        Some(Value::String(op)) if op == "between" && map.len() == 3 => {
            // Pointer recordings preserve half-open edges, not rounded inclusive cells.
            Ok(SelectionCommand::Between {
                start: point(map.get("start").ok_or_else(|| reject("start missing"))?)?,
                end: point(map.get("end").ok_or_else(|| reject("end missing"))?)?,
            })
        }
        Some(Value::String(op)) if op == "range" && map.len() == 4 => {
            // Unknown expansion modes are invalid input, not a fallback to simple drag.
            let kind = match map.get("kind") {
                Some(Value::String(k)) => match k.as_str() {
                    "simple" => SelectionKind::Simple,
                    "word" => SelectionKind::Word,
                    "line" => SelectionKind::Line,
                    "block" => SelectionKind::Block,
                    "semantic" => SelectionKind::Semantic,
                    _ => return Err(reject("unknown selection kind")),
                },
                _ => return Err(reject("selection kind missing")),
            };
            // Decode both endpoints through the same JS-exact validation seam.
            Ok(SelectionCommand::Range {
                kind,
                start: point(map.get("start").ok_or_else(|| reject("start missing"))?)?,
                end: point(map.get("end").ok_or_else(|| reject("end missing"))?)?,
            })
        }
        Some(Value::String(op)) if op == "commandOutput" && map.len() == 2 => {
            // Distinguish an explicit null from an omitted semantic position.
            let at = map.get("at").ok_or_else(|| reject("at missing"))?;
            Ok(SelectionCommand::CommandOutput(
                if matches!(at, Value::Null) {
                    None
                } else {
                    Some(point(at)?)
                },
            ))
        }
        Some(Value::String(op)) if map.len() == 1 && op == "all" => Ok(SelectionCommand::All),
        Some(Value::String(op)) if map.len() == 1 && op == "clear" => Ok(SelectionCommand::Clear),
        _ => Err(reject("unknown selection operation or unexpected fields")),
    }
}
/// Facts carry a nullable range without an independently mutable text copy.
pub(super) fn range_value(range: Option<SelectionRange>) -> Value {
    // A cleared selection is a fact with null range, not an omitted event.
    let range = range.map_or(Value::Null, |r| {
        // A position contains identity and column, with no viewport-dependent row number.
        Value::Map(BTreeMap::from([
            ("start".into(), point_value(r.start)),
            ("end".into(), point_value(r.end)),
            ("block".into(), Value::Bool(r.block)),
        ]))
    });
    // A position contains identity and column, with no viewport-dependent row number.
    Value::Map(BTreeMap::from([("range".into(), range)]))
}
/// Validates a fact's exact range shape for replay and external subscribers.
pub(super) fn parse_range(value: &Value) -> Result<Option<SelectionRange>, ValidationReport> {
    // Reject scalar/list payloads before any map field lookup or numeric cast.
    let Value::Map(map) = value else {
        return Err(reject("selection fact must be a map"));
    };
    // A fact has only range; copied text remains an explicit contents query.
    if map.len() != 1 {
        return Err(reject("selection fact requires range"));
    }
    // Block is mandatory because it changes extraction and fill semantics.
    match map.get("range") {
        Some(Value::Null) => Ok(None),
        Some(Value::Map(r)) if r.len() == 3 => {
            let Some(Value::Bool(block)) = r.get("block") else {
                return Err(reject("block must be Boolean"));
            };
            Ok(Some(SelectionRange {
                start: point(r.get("start").ok_or_else(|| reject("start missing"))?)?,
                end: point(r.get("end").ok_or_else(|| reject("end missing"))?)?,
                block: *block,
            }))
        }
        _ => Err(reject("invalid selection range")),
    }
}

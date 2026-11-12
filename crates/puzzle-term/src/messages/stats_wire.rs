//! Checked wire counters for opt-in completed-frame statistics (design §5.7).
//! Timing values are finite nonnegative milliseconds; counts are exact integers.
//! This conversion runs only for subscribers, never per cell or parser byte.
//!
//! Phase order is fixed: cache matching, row rebuilding, assembly, geometry,
//! overlay compilation and uploads. A wire peer must supply every phase so
//! recordings cannot silently shift an old phase into a newly appended slot.
//! Instance order is likewise fixed: rectangle, vector, raster. Bytes count
//! both vector textures and raster pages; no raw glyph or terminal text leaks.
//!
//! JavaScript represents numbers as doubles. An integer larger than 2^53-1
//! cannot round-trip exactly, even on a 64-bit native host, so the decoder
//! applies both the wire range and the current target's usize range.
//! Decoder errors carry validation reports through the ordinary bus boundary.
use super::{FrameStats, Value};
use puzzle_bus::{value::ValidationReport, wire::reject};
use std::collections::BTreeMap;

/// Encodes coarse phase and pipeline arrays with explicit units in field names.
pub(super) fn encode(stats: &FrameStats) -> Value {
    // Stable camelCase keys distinguish milliseconds from bytes and byte rates.
    Value::Map(BTreeMap::from([
        (
            "cpuMs".into(),
            Value::List(stats.cpu_ms.map(Value::Number).into()),
        ),
        (
            "instances".into(),
            Value::List(stats.instances.map(|n| Value::Number(n as f64)).into()),
        ),
        ("glyphBytes".into(), Value::Number(stats.glyph_bytes as f64)),
        (
            "uploadBytes".into(),
            Value::Number(stats.upload_bytes as f64),
        ),
        (
            "parseBytesPerSecond".into(),
            Value::Number(stats.parse_bytes_per_second),
        ),
    ]))
}
/// Rejects wrong lengths, extra fields, negative rates and fractional counts.
pub(super) fn decode(value: &Value) -> Result<FrameStats, ValidationReport> {
    let Value::Map(map) = value else {
        return Err(reject("frame statistics must be a map"));
    };
    // Exact field count rejects extensions until the contract deliberately versions them.
    if map.len() != 5 {
        return Err(reject("frame statistics require exactly five fields"));
    }
    // Fixed phase and pipeline order is part of the stable frame-statistics contract.
    let phases = array::<6>(map.get("cpuMs"))?;
    let counts = array::<3>(map.get("instances"))?;
    // Validate integer counts after validating each numeric value for finiteness.
    let mut instances = [0; 3];
    for (target, value) in instances.iter_mut().zip(counts) {
        *target = count(value)?;
    }
    // Required named counters are looked up separately; an unknown key cannot substitute.
    Ok(FrameStats {
        cpu_ms: phases,
        instances,
        glyph_bytes: count(number(map.get("glyphBytes"))?)?,
        upload_bytes: count(number(map.get("uploadBytes"))?)?,
        parse_bytes_per_second: number(map.get("parseBytesPerSecond"))?,
    })
}
/// Fixed arrays prevent recordings from silently changing phase correspondence.
fn array<const N: usize>(value: Option<&Value>) -> Result<[f64; N], ValidationReport> {
    // An absent field and a non-list field are equally invalid at this boundary.
    let Some(Value::List(list)) = value else {
        return Err(reject("frame counters must be lists"));
    };
    // Never truncate extras or zero-fill a partial frame report.
    if list.len() != N {
        return Err(reject("frame counter list has wrong length"));
    }
    // A fixed stack array avoids another allocation while decoding the list.
    let mut output = [0.0; N];
    for (target, value) in output.iter_mut().zip(list) {
        *target = number(Some(value))?;
    }
    Ok(output)
}
/// NaN, infinity and negative counters never enter the typed event lane.
fn number(value: Option<&Value>) -> Result<f64, ValidationReport> {
    // Reject special floating values before downstream formatting or rate arithmetic.
    if let Some(Value::Number(value)) = value
        && value.is_finite()
        && *value >= 0.0
    {
        return Ok(*value);
    }
    // Fractional or overflowing counts would break replay equivalence across targets.
    Err(reject("frame counter must be finite and nonnegative"))
}
/// Integers must fit this target and the exact JSON-number representation.
fn count(value: f64) -> Result<usize, ValidationReport> {
    // The strict inequality excludes usize::MAX rounded up to the next power of two.
    if value.fract() == 0.0 && value < usize::MAX as f64 && value <= 9_007_199_254_740_991.0 {
        return Ok(value as usize);
    }
    // Fractional or overflowing counts would break replay equivalence across targets.
    Err(reject("frame count must be an exact representable integer"))
}

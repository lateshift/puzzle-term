//! Small validated wire maps for VT facts and host actions (§5.7).
//!
//! The bus stores concrete Rust types. These maps exist only at the public
//! boundary, where a recorder or external subscriber needs plain data.
//! All numeric fields require finite integral values within their Rust
//! ranges. An invalid source returns a report instead of panicking or
//! wrapping an oversized JS number into a different grid dimension (R8).
//!
//! OSC 133 D may omit its exit code. Explicit null preserves that absence
//! through a recording; zero is a real successful exit status. Duration is
//! reserved as null until the Session owns timestamped shell zones.
//! A notification keeps title and body separately so policy can inspect
//! them before any host UI receives the request.
//!
//! Each parser verifies its outer map before touching a field. This matters
//! for external JS and JSON callers, which can supply null, lists or deeply
//! nested values in place of a terminal fact. The shared tree validator
//! bounds nesting and strings before these shape-specific checks run.
//! `pair` produces deterministic map ordering through BTreeMap, giving a
//! stable recording byte stream regardless of hash-table iteration order.
//! The grid decoder limits dimensions to u16 because the native PTY API
//! ultimately reports u16 rows and columns; accepting larger values here
//! would only fail after interceptors had already observed the command.
//! Progress is limited to the ConEmu state range and a percentage of 100,
//! mirroring VT's own bounds instead of changing a fact during replay.
use super::ExitInfo;
use puzzle_bus::{Value, value::ValidationReport, wire::reject};
use std::collections::BTreeMap;

/// Creates a two-field value map without exposing stringly typed bus state.
fn pair(a: (&str, Value), b: (&str, Value)) -> Value {
    // Only the wire boundary allocates a map. The ordinary typed bus
    // dispatch never constructs these fields (design §5.7, P8).
    let mut map = BTreeMap::new();
    map.insert(a.0.into(), a.1);
    map.insert(b.0.into(), b.1);
    Value::Map(map)
}

/// Encodes process status without conflating a signal and an exit code.
pub(super) fn exit_info(info: &ExitInfo) -> Value {
    // The three named fields keep recorded lifecycle entries stable when a
    // process has no numeric code because it ended by signal.
    let mut map = BTreeMap::new();
    map.insert(
        "code".into(),
        info.code
            .map_or(Value::Null, |code| Value::Number(f64::from(code))),
    );
    map.insert(
        "signal".into(),
        info.signal
            .as_ref()
            .map_or(Value::Null, |name| Value::String(name.clone())),
    );
    // Runtime is part of the policy decision during replay, so it travels
    // with the status rather than being recomputed from the replay clock.
    map.insert("runtimeMs".into(), Value::Number(info.runtime_ms as f64));
    Value::Map(map)
}

/// Validates a recorded public exit fact before it reaches an observer.
pub(super) fn parse_exit(value: &Value) -> Result<ExitInfo, ValidationReport> {
    // A scalar or array cannot carry the mutually exclusive code and signal
    // fields, and accepting one would make malformed records ambiguous.
    let Value::Map(map) = value else {
        return Err(reject("exit status must be a map"));
    };
    // Float-to-int casts saturate in Rust; check the integral i32 range first
    // so an external status cannot be silently rewritten.
    let code = match map.get("code") {
        Some(Value::Null) => None,
        Some(Value::Number(number))
            if number.is_finite()
                && number.fract() == 0.0
                && *number >= f64::from(i32::MIN)
                && *number <= f64::from(i32::MAX) =>
        {
            Some(*number as i32)
        }
        _ => return Err(reject("exit code must be i32 or null")),
    };
    let signal = match map.get("signal") {
        Some(Value::Null) => None,
        Some(Value::String(name)) => Some(name.clone()),
        _ => return Err(reject("exit signal must be a string or null")),
    };
    // JSON numbers represent integers exactly only through 2^53 - 1.
    let runtime_ms = integer(map.get("runtimeMs"), 0, (1_u64 << 53) - 1)?;
    Ok(ExitInfo {
        code,
        signal,
        runtime_ms,
    })
}

/// Makes OSC 9/777's two sanitized strings visible to a policy adapter.
pub(super) fn notification(title: &str, body: &str) -> Value {
    pair(
        ("title", Value::String(title.into())),
        ("body", Value::String(body.into())),
    )
}

/// Rejects notifications missing either text field.
pub(super) fn parse_notification(value: &Value) -> Result<(String, String), ValidationReport> {
    // Host policy needs both fields before it can decide whether to show
    // a notification; partial payloads never reach its interceptor.
    let Value::Map(map) = value else {
        return Err(reject("notification must be a map"));
    };
    let (Some(Value::String(title)), Some(Value::String(body))) =
        (map.get("title"), map.get("body"))
    else {
        return Err(reject("notification needs title and body strings"));
    };
    Ok((title.clone(), body.clone()))
}

/// Preserves a missing OSC 133 exit code as null rather than success.
pub(super) fn finished(exit: Option<i32>) -> Value {
    pair(
        (
            "exit",
            exit.map_or(Value::Null, |code| Value::Number(f64::from(code))),
        ),
        ("duration", Value::Null),
    )
}

/// Decodes an optional exact i32 exit status from a shell fact.
pub(super) fn parse_finished(value: &Value) -> Result<Option<i32>, ValidationReport> {
    // Duration is an explicit reserved null. A non-null external value
    // would pretend the session had measured time it does not yet own.
    let Value::Map(map) = value else {
        return Err(reject("command finish must be a map"));
    };
    if !matches!(map.get("duration"), Some(Value::Null)) {
        return Err(reject("command duration is not available yet"));
    }
    match map.get("exit") {
        // A missing status and a zero status are semantically different
        // for shell integration badges and recordings.
        Some(Value::Null) => Ok(None),
        Some(Value::Number(number))
            if number.is_finite()
                && number.fract() == 0.0
                && *number >= f64::from(i32::MIN)
                && *number <= f64::from(i32::MAX) =>
        {
            Ok(Some(*number as i32))
        }
        _ => Err(reject("exit status must be i32 or null")),
    }
}

/// Reports visible grid dimensions as exact nonzero integers.
pub(super) fn grid(columns: usize, rows: usize) -> Value {
    pair(
        ("columns", Value::Number(columns as f64)),
        ("rows", Value::Number(rows as f64)),
    )
}

/// Rejects impossible or fractional grid sizes at the public boundary.
pub(super) fn parse_grid(value: &Value) -> Result<(usize, usize), ValidationReport> {
    // Two columns and one row are the VT core's minimum valid geometry;
    // do not let the wire expose a size that the core must re-clamp.
    let Value::Map(map) = value else {
        return Err(reject("grid size must be a map"));
    };
    let columns = integer(map.get("columns"), 2, u16::MAX as u64)?;
    let rows = integer(map.get("rows"), 1, u16::MAX as u64)?;
    Ok((columns as usize, rows as usize))
}

/// Keeps ConEmu progress state separate from its percentage.
pub(super) fn progress(state: u8, percent: u8) -> Value {
    pair(
        ("state", Value::Number(f64::from(state))),
        ("percent", Value::Number(f64::from(percent))),
    )
}

/// Bounds progress values before a host paints an indicator.
pub(super) fn parse_progress(value: &Value) -> Result<(u8, u8), ValidationReport> {
    // Bounds match the VT parser's OSC 9;4 clamp, making replay exact.
    let Value::Map(map) = value else {
        return Err(reject("progress must be a map"));
    };
    let state = integer(map.get("state"), 0, 4)? as u8;
    let percent = integer(map.get("percent"), 0, 100)? as u8;
    Ok((state, percent))
}

/// Checks one JSON number before narrowing it to a host integer.
fn integer(value: Option<&Value>, min: u64, max: u64) -> Result<u64, ValidationReport> {
    // Cast only after finiteness, integrality and bounds checks; Rust's
    // saturating float cast would silently rewrite a hostile number.
    let Some(Value::Number(number)) = value else {
        return Err(reject("numeric field missing"));
    };
    if !number.is_finite() || number.fract() != 0.0 || *number < min as f64 || *number > max as f64
    {
        return Err(reject("numeric field outside permitted range"));
    }
    Ok(*number as u64)
}

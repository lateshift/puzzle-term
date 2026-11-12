//! JSON conversion at the native platform boundary (§14.1, D-23).
//!
//! JSON has no byte scalar; config.json therefore accepts only the ordinary
//! null, bool, number, string, list and map subset of the bus Value tree.
//! A depth bound is enforced while converting so hostile files cannot force
//! a recursive drop deeper than the wire validator permits.
use puzzle_term::settings::SettingsValue as Value;
use serde_json::Value as Json;
use std::collections::BTreeMap;
/// Generates the native editor schema from the same Rust declarations.
pub(super) mod schema;

/// Converts a CLI scalar or JSON fragment into the same neutral value tree.
pub(super) fn cli_value(text: &str) -> Result<Value, String> {
    // A typed JSON scalar lets `--set font.size=15` and
    // `--set cursor.blink=false` enter the same validators as config.json.
    match serde_json::from_str::<Json>(text) {
        Ok(json) => from_json(&json, 0),
        // Bare enum and path names are common CLI choices; a quoted JSON
        // string remains available when the text starts with punctuation.
        Err(_) => Ok(Value::String(text.into())),
    }
}

/// Bounds recursion before validation and rejects unrepresentable numbers.
pub fn from_json(json: &Json, depth: usize) -> Result<Value, String> {
    // Reject before recursing into a child: a deeply nested user file
    // must not make either conversion or destruction exhaust the stack.
    if depth > 64 {
        return Err("settings JSON exceeds 64 nested levels".into());
    }
    match json {
        Json::Null => Ok(Value::Null),
        Json::Bool(value) => Ok(Value::Bool(*value)),
        // The bus uses finite f64 numbers. A JSON integer outside that
        // representation cannot be silently rounded into a setting.
        Json::Number(number) => {
            // Integers beyond f64's exact range cannot round-trip in the
            // neutral tree, including under an unknown future key.
            const EXACT: i64 = 1_i64 << 53;
            if number
                .as_i64()
                .is_some_and(|value| !(-EXACT..=EXACT).contains(&value))
                || number.as_u64().is_some_and(|value| value > EXACT as u64)
            {
                return Err("settings integer exceeds exact wire range".into());
            }
            number
                .as_f64()
                .filter(|value| value.is_finite())
                .map(Value::Number)
                .ok_or("settings number is not finite".into())
        }
        Json::String(value) => Ok(Value::String(value.clone())),
        // Lists preserve order because font fallbacks and bindings have
        // precedence determined by their position in the source file.
        Json::Array(values) => values
            .iter()
            .map(|value| from_json(value, depth + 1))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::List),
        // BTreeMap gives deterministic validation reports and exports,
        // independent of serde_json's object storage representation.
        Json::Object(values) => values
            .iter()
            .map(|(key, value)| Ok((key.clone(), from_json(value, depth + 1)?)))
            .collect::<Result<BTreeMap<_, _>, String>>()
            .map(Value::Map),
    }
}

/// Reconstructs JSON without a derive dependency on neutral crates.
pub(super) fn to_json(value: &Value) -> Json {
    // This path exports the user layer only. The native adapter never
    // creates Bytes, but keep the match total for future bus-tree callers.
    match value {
        Value::Null => Json::Null,
        Value::Bool(value) => Json::Bool(*value),
        // Validated settings numbers are finite; Null is a defensive
        // result if a Rust caller constructs an invalid tree directly.
        Value::Number(value) => serde_json::Number::from_f64(*value)
            .map(Json::Number)
            .unwrap_or(Json::Null),
        Value::String(value) => Json::String(value.clone()),
        Value::Bytes(_) => Json::Null,
        Value::List(values) => Json::Array(values.iter().map(to_json).collect()),
        // Unknown nested user keys pass through without schema knowledge,
        // allowing an older executable to save a newer config safely.
        Value::Map(values) => Json::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), to_json(value)))
                .collect(),
        ),
    }
}

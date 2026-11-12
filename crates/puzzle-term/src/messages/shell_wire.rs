//! Command labels are optional bounded data, never a request to execute them.
//!
//! Every embedder receives the same map through the public fact catalogue. A
//! missing label is null, rather than a guessed title or reconstructed grid text.
//! Bare shell markers become a map with a null label at the bus boundary.
//! Native hosts need no OSC decoder; web/recording users share the same payload.
//! Validation rejects hostile recorded values instead of changing their meaning.
//! The parser owns truncation; external wire input must already fit its byte cap.
use puzzle_bus::{Value, value::ValidationReport, wire::reject};
use std::collections::BTreeMap;

/// Preserve unavailable command text as null, including old bare C marks.
pub(super) fn started(command: Option<&str>) -> Value {
    // Allocate only at the wire boundary; the typed event keeps its owned string.
    Value::Map(BTreeMap::from([(
        "command".into(),
        command.map_or(Value::Null, |value| Value::String(value.into())),
    )]))
}

/// External recordings cannot bypass the command-label byte bound.
pub(super) fn parse_started(value: &Value) -> Result<Option<String>, ValidationReport> {
    if value == &Value::Null {
        // Old action recordings carry no label but retain their start boundary.
        return Ok(None);
    }
    let Value::Map(map) = value else {
        return Err(reject("command start needs a map"));
    };
    // Null explicitly distinguishes unsupported shell integration from an empty
    // entered command. Controls are not safe labels for native chrome/tooltips.
    if map.len() != 1 {
        return Err(reject("command start needs exactly one label field"));
    }
    match map.get("command") {
        Some(Value::Null) => Ok(None),
        Some(Value::String(command))
            if command.len() <= puzzle_vt::MAX_COMMAND_BYTES
                && !command.chars().any(char::is_control) =>
        {
            Ok(Some(command.clone()))
        }
        _ => Err(reject("command must be null or bounded control-free UTF-8")),
    }
}

/// Keep command payload validation separate from the general event dispatcher.
pub(super) fn event(value: &Value) -> Result<super::Event, ValidationReport> {
    // Missing labels remain explicit data; malformed external maps are rejected.
    Ok(super::Event::CommandStarted(parse_started(value)?))
}

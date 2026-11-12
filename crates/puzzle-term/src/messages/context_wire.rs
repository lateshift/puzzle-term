//! Lossless UAPI context snapshots at the plain-data event boundary (§5.7).
//! Context numbers stay decimal strings so JS/JSON never round a 64-bit identity.
//! Replay validates depth, IDs and every decoded field before creating a fact.
//! The outer envelope describes a complete path, not an incremental start/end.
//! Each entry is checked independently before the path's unique-ID invariant.
//! Validation failures return a report; malformed recordings cannot panic a host.
//! No decoding occurs in frame building or ordinary terminal output.
use puzzle_bus::{Value, value::ValidationReport, wire::reject};
use puzzle_vt::context::{Context, Field, FieldKey, MAX_DEPTH, MAX_ID};
use std::collections::BTreeMap;

/// Copies only when an external subscriber/recorder asks for the wire value.
pub(super) fn encode(contexts: &[Context]) -> Value {
    // Keep root-to-leaf order: a map keyed by ID would lose the active leaf.
    let list = Value::List(
        contexts
            .iter()
            .map(|context| {
                // Ordered maps make recordings deterministic, independently of field order.
                // Preserve decoded strings verbatim, including literal backslashes.
                let fields = context
                    .fields
                    .iter()
                    .map(|f| (f.key.name().into(), Value::String(f.value.clone())))
                    .collect();
                // Every entry has the same explicit identity/metadata envelope.
                Value::Map(BTreeMap::from([
                    ("id".into(), Value::String(context.id.clone())),
                    ("fields".into(), Value::Map(fields)),
                ]))
            })
            .collect(),
    );
    // An empty list is an observable end-of-session state, not a missing event.
    Value::Map(BTreeMap::from([("contexts".into(), list)]))
}

/// Reject malformed facts rather than repairing recorded metadata silently.
pub(super) fn decode(value: &Value) -> Result<Vec<Context>, ValidationReport> {
    // Reject alternate envelope shapes instead of silently ignoring a bad record.
    let Value::Map(map) = value else {
        return Err(reject("context fact must be a map"));
    };
    // Strict replay catches misspelled keys that a permissive parser would hide.
    if map.len() != 1 {
        return Err(reject("unexpected context fact fields"));
    }
    // The list order is significant even if metadata maps have sorted keys.
    let Some(Value::List(list)) = map.get("contexts") else {
        return Err(reject("contexts must be a list"));
    };
    // Check the path bound before allocating any decoded entries.
    if list.len() > MAX_DEPTH {
        return Err(reject("context depth exceeds bound"));
    }
    let mut contexts: Vec<Context> = Vec::new();
    for item in list {
        let context = decode_context(item)?;
        // Repeated IDs cannot describe a valid active path; starts would truncate it.
        if contexts.iter().any(|c| c.id == context.id) {
            return Err(reject("invalid or duplicate context ID"));
        }
        contexts.push(context);
    }
    Ok(contexts)
}

/// Decode one owned entry; the caller checks its relation to other path entries.
/// Identity and fields are mandatory even for a context without metadata.
/// Null is not shorthand for an empty map, because recordings must be exact.
/// Field names are sorted by Value's map; the returned vector retains that order.
fn decode_context(value: &Value) -> Result<Context, ValidationReport> {
    // The OSC parser accepts malformed optional fields leniently; recorded facts
    // already represent validated state and must round-trip without repairs.
    let Value::Map(map) = value else {
        return Err(reject("context must be a map"));
    };
    // IDs use strings even if an application happened to choose only digits.
    let Some(Value::String(id)) = map.get("id") else {
        return Err(reject("context ID missing"));
    };
    let Some(Value::Map(fields)) = map.get("fields") else {
        return Err(reject("context fields missing"));
    };
    // IDs are decoded ASCII here: escape spellings are not interpreted twice.
    if map.len() != 2
        || id.is_empty()
        || id.len() > MAX_ID
        || !id.bytes().all(|b| (32..=126).contains(&b))
    {
        return Err(reject("invalid context ID"));
    }
    let mut metadata = Vec::new();
    for (name, value) in fields {
        // Closed names bound retained metadata to the fifteen UAPI start fields.
        let Some(key) = FieldKey::parse(name) else {
            return Err(reject("unknown context field"));
        };
        // Numeric JSON would round large PIDs; their wire representation is text.
        let Value::String(text) = value else {
            return Err(reject("context field must be lossless text"));
        };
        // Reject oversized text before copying an untrusted recording's string.
        // Four bytes is UTF-8's maximum scalar size; no valid field exceeds this.
        if text.len() > puzzle_vt::context::MAX_TEXT * 4 {
            return Err(reject("context field exceeds string bound"));
        }
        // OSC and replay share decoded-value validation, including string bounds.
        let Some(field) = Field::new(key, text.clone()) else {
            return Err(reject("invalid context field"));
        };
        // BTreeMap iteration supplies the same canonical order as the OSC parser.
        metadata.push(field);
    }
    // Unknown metadata is rejected rather than retained behind an opaque key.
    // Only validated entries enter the owned event payload.
    Ok(Context {
        id: id.clone(),
        fields: metadata,
    })
}

#[cfg(test)]
mod tests;

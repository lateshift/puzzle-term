//! UAPI.15 hierarchical context signalling (OSC 3008), version 1.0.
//! Reference: https://uapi-group.org/specifications/specs/osc_context/
//! This module interprets wire fields; Terminal owns the active path outside
//! origin modules. Unknown/invalid fields do not discard other valid metadata.
//! RIS preserves contexts; only explicit end or session teardown closes them.
//! The stack is auxiliary information and cannot authorize an OS action.
//! Parsing an end never depends on optional exit metadata being valid.
//! Start metadata is a set: wire order has no meaning, and omitted fields reset.
//! Retention and its overflow policy belong to Terminal, not this grammar module.
//! The raw observer preserves fields beyond vte's fixed parameter table.
mod fields;
use crate::{
    context::{Context, Field},
    terminal::Terminal,
};

/// Apply a complete, bounded raw string after vte recognizes OSC 3008.
pub(crate) fn dispatch(term: &mut Terminal, raw: &[u8]) {
    // A truncated start must not replace a context with incomplete metadata.
    if raw.len() >= crate::osc::MAX_BUFFER_BYTES {
        return;
    }
    let Some(body) = raw.strip_prefix(b"3008;") else {
        return;
    };
    let mut parts = body.split(|b| *b == b';');
    let Some(first) = parts.next() else {
        return;
    };
    let (start, id) = if let Some(id) = first.strip_prefix(b"start=") {
        (true, id)
    } else if let Some(id) = first.strip_prefix(b"end=") {
        (false, id)
    } else {
        return;
    };
    // ID corruption invalidates the operation, not the following terminal output.
    let Some(id) = fields::id(id) else {
        return;
    };
    if !start {
        // End metadata is auxiliary to historical output; this API retains only
        // active contexts. Invalid exit/status/signal fields cannot block an end.
        term.end_context(&id);
        return;
    }
    // Replacement starts have fresh defaults: nothing is inherited from the old ID.
    let mut metadata: Vec<Field> = Vec::new();
    for part in parts {
        if let Some(field) = fields::parse(part) {
            // Last valid duplicate wins; malformed duplicates do not erase valid data.
            if let Some(old) = metadata.iter_mut().find(|f| f.key == field.key) {
                *old = field;
            } else {
                metadata.push(field);
            }
        }
    }
    // Metadata is a key/value set. Canonical order makes typed and wire replay
    // identical even though a program may send the keys in any order.
    metadata.sort_unstable_by_key(|field| field.key.name());
    term.start_context(Context {
        id,
        fields: metadata,
    });
}

//! Binary MIME clipboard writes preserve the existing public clipboard policy kind.
//! Correlation tokens are decimal strings so JavaScript cannot round large identities.
//! The public clipboard/write kind predates binary clipboard transactions.
//! Its text/HTML shape remains accepted so existing clients keep working.
//! The MIME shape contains exactly token and mime, with no extra fields.
//! Each alternative contains exactly type and binary data, not base64 strings.
//! Value::Bytes keeps the bus representation independent of OSC wire encoding.
//! Canonical decimal tokens preserve all 64 bits across JSON-like host bridges.
//! Leading zeros, signs and other ambiguous token spellings are rejected.
//! The total byte allowance is checked before cloning data into owned requests.
//! Every name is printable ASCII without whitespace, bounded to 256 bytes.
//! The count limit bounds empty alternatives as well as nonempty payloads.
//! Validation errors return the existing bus report rather than panicking.
//! No wire field can forge Origin::Terminal or create a trusted user paste gesture.
//! External dispatch still enters the normal clipboard write interceptor chain.
//! Token zero is the public API write path, with no OSC client waiting for a reply.
//! Nonzero tokens correlate existing terminal requests after policy rewrites.
//! Serialization is observer-driven; an unobserved command builds no Value tree.
//! The retained Arc-backed representations are copied only at this wire boundary.
//! The host does not receive a password through this command shape.
//! Alternative maps reject unknown fields instead of silently retaining them.
//! The aggregate limit includes every separately owned binary representation.
//! An invalid item aborts the entire conversion, never a prefix of the list.
//! No partial native write can escape from this conversion helper.
use super::{ClipboardContent, Command};
use crate::clipboard::{MAX_BYTES, MAX_TYPES, MimeData};
use puzzle_bus::{Value, value::ValidationReport, wire::reject};
use std::collections::BTreeMap;
/// Serializes alternatives lazily, only when a wire observer explicitly requests them.
pub(super) fn to_value(token: u64, items: &[MimeData]) -> Value {
    Value::Map(BTreeMap::from([
        ("token".into(), Value::String(token.to_string())),
        (
            "mime".into(),
            Value::List(
                items
                    .iter()
                    .map(|item| {
                        Value::Map(BTreeMap::from([
                            ("type".into(), Value::String(item.mime.clone())),
                            ("data".into(), Value::Bytes(item.data.to_vec())),
                        ]))
                    })
                    .collect(),
            ),
        ),
    ]))
}
/// Legacy HTML remains unchanged; binary alternatives have one exact validated shape.
pub(super) fn from_value(value: &Value) -> Result<Command, ValidationReport> {
    let Value::Map(map) = value else {
        return Err(reject("clipboard write requires a map"));
    };
    // Dispatch legacy shapes before requiring fields specific to the MIME variant.
    if !map.contains_key("mime") {
        return super::copy_wire::html_command(value);
    }
    // Exact keys prevent silently accepting misspelled or future privileged fields.
    if map.len() != 2 {
        return Err(reject("MIME write requires token and mime"));
    }
    let Some(Value::String(token)) = map.get("token") else {
        return Err(reject("clipboard token missing"));
    };
    // Parse as an integer, then check its unique canonical decimal spelling.
    let parsed = token
        .parse::<u64>()
        .map_err(|_| reject("invalid clipboard token"))?;
    if parsed.to_string() != *token {
        return Err(reject("noncanonical clipboard token"));
    }
    let Some(Value::List(values)) = map.get("mime") else {
        return Err(reject("clipboard mime list missing"));
    };
    if values.len() > MAX_TYPES {
        return Err(reject("too many clipboard alternatives"));
    }
    // Accumulate into temporary ownership; validation failure publishes nothing.
    let mut items = Vec::new();
    let mut bytes = 0usize;
    for value in values {
        let Value::Map(item) = value else {
            return Err(reject("MIME item requires a map"));
        };
        let (Some(Value::String(mime)), Some(Value::Bytes(data))) =
            (item.get("type"), item.get("data"))
        else {
            return Err(reject("MIME item requires type and data"));
        };
        // Saturation rejects malicious aggregate overflow rather than wrapping small.
        bytes = bytes.saturating_add(data.len());
        if item.len() != 2
            || mime.is_empty()
            || mime.len() > 256
            || !mime.bytes().all(|b| b.is_ascii_graphic())
            || bytes > MAX_BYTES
        {
            return Err(reject("invalid or oversized MIME clipboard data"));
        }
        // Copy only after all metadata and aggregate limits for this item pass.
        items.push(MimeData {
            mime: mime.clone(),
            data: data.clone().into(),
        });
    }
    Ok(Command::ClipboardWrite(ClipboardContent::Mime {
        token: parsed,
        items,
    }))
}

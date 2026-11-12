//! Exact clipboard request shapes for external dispatch and recordings (§5.7).
//!
//! The source/format enum stays typed inside Session. Maps exist only at this
//! boundary, where unknown fields and unsupported formats are reported before
//! any clipboard side effect. Origins come from the bus, never this payload.
//!
//! Selection copy supports text, VT and HTML. Command-output copy has the single
//! plain representation named by the action catalogue; a forged HTML format is
//! rejected rather than silently changing the source. Both are coarse requests.
//! The later write carries either legacy string text or an html/text pair.
//! Requiring both strings makes rich paste and plain paste one policy decision.
//! Removing the alternative at the wire boundary is a validation failure.
//! Explicit block-copy identities are strings because JS numbers lose u64 bits.
//! A missing identity is rejected, never interpreted as the newest command.
//! The source and representation remain separate so policy can inspect both.
//! Successful decoding still grants no clipboard permission: dispatch runs policy.
//! An evicted command is resolved as absent by the Session's later extraction.
//! There is no text payload here that could bypass the normal source lookup.

use crate::{selection::CopyTarget, vt::view::Format};
use puzzle_bus::{Value, value::ValidationReport, wire::reject};
use std::collections::BTreeMap;

/// Encodes one typed request with stable source and format names.
pub(super) fn to_value(target: CopyTarget) -> Value {
    // Command output has one defined plain-text representation.
    let (source, format) = match target {
        CopyTarget::CommandOutput => ("commandOutput", "text"),
        CopyTarget::ZoneOutput(_) => ("zoneOutput", "text"),
        CopyTarget::Selection(format) => (
            "selection",
            match format {
                Format::PlainText => "text",
                Format::Vt => "vt",
                Format::Html => "html",
            },
        ),
    };
    // These names are stable replay data, independent of enum discriminants.
    let mut map = BTreeMap::from([
        ("source".into(), Value::String(source.into())),
        ("format".into(), Value::String(format.into())),
    ]);
    // A decimal identity remains exact when a recording passes through a JS host.
    if let CopyTarget::ZoneOutput(id) = target {
        map.insert("zone".into(), Value::String(id.to_string()));
    }
    Value::Map(map)
}

/// Rejects malformed or unsupported sources without silently copying another range.
pub(super) fn from_value(value: &Value) -> Result<CopyTarget, ValidationReport> {
    let Value::Map(map) = value else {
        return Err(reject("copy requires a map"));
    };
    // Cardinality and required-key lookups together reject extras and omissions.
    if map.len() != 2 && map.len() != 3 {
        return Err(reject(
            "copy requires source and format, and zone for zoneOutput",
        ));
    }
    // Source strings are required: null cannot silently mean selection.
    let Some(Value::String(source)) = map.get("source") else {
        return Err(reject("copy source missing"));
    };
    // An explicit representation prevents guessed rich clipboard contents.
    let Some(Value::String(format)) = map.get("format") else {
        return Err(reject("copy format missing"));
    };
    // Restrict formats before checking source/format combinations.
    let format = match format.as_str() {
        "text" => Format::PlainText,
        "vt" => Format::Vt,
        "html" => Format::Html,
        _ => return Err(reject("unknown copy format")),
    };
    // The third field is legal only for an explicit command identity.
    if source == "zoneOutput" && format == Format::PlainText && map.len() == 3 {
        // Decimal strings preserve every u64 identity across JavaScript's numeric limit.
        if let Some(Value::String(text)) = map.get("zone")
            && let Ok(id) = text.parse::<u64>()
            // Canonical spelling rejects signs, padding and alternate integer syntax.
            && id.to_string() == *text
        {
            return Ok(CopyTarget::ZoneOutput(id));
        }
        return Err(reject("zone requires a canonical unsigned decimal string"));
    }

    // Neither selection nor latest-output requests may smuggle an unused zone key.
    if map.len() != 2 {
        return Err(reject("unexpected copy field"));
    }
    // OSC-output copy is plain by contract; do not reinterpret an invalid format.
    match source.as_str() {
        "selection" => Ok(CopyTarget::Selection(format)),
        "commandOutput" if format == Format::PlainText => Ok(CopyTarget::CommandOutput),
        _ => Err(reject("unknown copy source or unsupported output format")),
    }
}

/// Rich clipboard data has exactly the two representations required for paste.
pub(super) fn html_value(html: &str, text: &str) -> Value {
    // Keep plain and rich values in one payload; policy sees a single write.
    // Strings are owned so deferred hosts cannot outlive a borrowed selection.
    Value::Map(BTreeMap::from([
        ("html".into(), Value::String(html.into())),
        ("text".into(), Value::String(text.into())),
    ]))
}

/// Legacy text stays a string; rich writes reject missing alternatives and extras.
pub(super) fn html_command(value: &Value) -> Result<crate::messages::Command, ValidationReport> {
    let Value::Map(map) = value else {
        return Err(reject("clipboard write requires text or html/text map"));
    };
    // Exact cardinality excludes hidden extra clipboard representations.
    if map.len() != 2 {
        return Err(reject("HTML clipboard requires html and text"));
    }
    // Both keys must contain strings; absence is never a plain-tags fallback.
    // Validation finishes before either representation reaches a platform.
    match (map.get("html"), map.get("text")) {
        (Some(Value::String(html)), Some(Value::String(text))) => Ok(
            crate::messages::Command::ClipboardWrite(crate::messages::ClipboardContent::Html {
                html: html.clone(),
                text: text.clone(),
            }),
        ),
        _ => Err(reject("HTML clipboard requires string html and text")),
    }
}

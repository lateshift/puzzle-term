//! Decode UAPI's two escapes and validate each metadata field independently.
//! Printable UTF-8 text is accepted as the protocol prose requires; IDs remain
//! printable ASCII. Limits apply after decoding, so escapes cannot hide growth.
//! Numeric identities retain their text to avoid JSON's 53-bit precision loss.
//! Escapes are decoded before checking length and allowed characters.
//! A malformed escape rejects its field, never repairs an application identity.
use crate::context::{Field, FieldKey, MAX_ID, MAX_TEXT};

/// Unescape a field without repairing malformed bytes or removing controls.
fn text(raw: &[u8], limit: usize, empty: bool) -> Option<String> {
    // A valid escape expands one decoded byte into four wire bytes.
    if raw.len() > limit.saturating_mul(4) {
        return None;
    }
    // Reject ill-formed UTF-8 rather than replacing bytes in context metadata.
    let source = std::str::from_utf8(raw).ok()?;
    let mut out = String::new();
    let mut rest = source;
    while let Some(at) = rest.find('\\') {
        // Copy only the unescaped prefix; the escape itself is handled once.
        out.push_str(&rest[..at]);
        let escaped = &rest[at..];
        // No generic C escapes: only these two spellings belong to UAPI.15.
        if let Some(next) = escaped.strip_prefix("\\x3b") {
            out.push(';');
            rest = next;
        } else {
            let next = escaped.strip_prefix("\\x5c")?;
            out.push('\\');
            rest = next;
        }
    }
    // No backslash remains to decode; preserve the final literal suffix.
    out.push_str(rest);
    if (!empty && out.is_empty())
        || out.chars().count() > limit
        || out.chars().any(|ch| ch <= '\u{1f}' || ch == '\u{7f}')
    {
        return None;
    }
    Some(out)
}

/// IDs are limited by the decoded printable-ASCII identity, not escape spelling.
pub(super) fn id(raw: &[u8]) -> Option<String> {
    // The prose permits UTF-8 metadata, but IDs have a narrower printable range.
    let value = text(raw, MAX_ID, false)?;
    value
        .bytes()
        .all(|b| (32..=126).contains(&b))
        .then_some(value)
}

/// Ignore a malformed/unknown field while allowing subsequent fields to be used.
pub(super) fn parse(raw: &[u8]) -> Option<Field> {
    // Split the first equals only: a command line can contain more equals signs.
    let split = raw.iter().position(|b| *b == b'=')?;
    let key = FieldKey::parse(std::str::from_utf8(&raw[..split]).ok()?)?;
    let value = text(&raw[split + 1..], MAX_TEXT, key == FieldKey::Cmdline)?;
    // Type, decimal and ID128 syntax is shared with recorded-event decoding.
    Field::new(key, value)
}

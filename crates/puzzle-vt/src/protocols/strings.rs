//! Shared bounded OSC string decoding; no protocol semantics live here.
//! Families share sanitation while Session retains clipboard and notification policy.

/// Rejoins semicolons inside an OSC payload that vte split into fields.
pub(crate) fn payload(params: &[&[u8]], start: usize) -> String {
    let mut text = String::new();
    for (index, part) in params.iter().enumerate().skip(start) {
        if index > start {
            text.push(';');
        }
        text.push_str(&String::from_utf8_lossy(part));
    }
    text
}

/// Removes controls and caps a user-visible OSC string by Unicode scalars.
pub(crate) fn clean(text: String, max_chars: usize) -> String {
    // Unicode scalar count bounds displayed text; control removal prevents
    // a title or body from smuggling terminal controls into a host surface.
    text.chars()
        .filter(|ch| !ch.is_control())
        .take(max_chars)
        .collect()
}

/// Parses one small decimal OSC field without accepting negative values.
pub(crate) fn number(part: Option<&&[u8]>) -> Option<u16> {
    // Decimal parsing rejects signs and oversized fields without panicking.
    std::str::from_utf8(part?).ok()?.parse::<u16>().ok()
}

//! Kitty metadata validation before any clipboard state or native work changes.
//! Reference: <https://sw.kovidgoyal.net/kitty/clipboard/>.
//! Metadata is colon-separated key=value data; binary payload begins after the
//! next semicolon and is decoded elsewhere. Borrow operation/location bytes until
//! validation succeeds, then allocate only the fields retained by the request.
//! Unknown keys are forward-compatible and cannot acquire permissions.
//! The last duplicate key wins, consistent with the defining implementation.
//! IDs have a literal allowlist because they are echoed without base64 encoding.
//! MIME, human name and password instead use strict bounded base64 plus UTF-8.
//! A password without a nonempty human name is deliberately discarded.
//! Even a retained password has no authority until Session matches its grant.
//! No metadata string is logged, so credentials cannot escape through diagnostics.
use super::base64;
/// Borrowed fields keep parsing independent of binary payload allocation.
pub(super) struct Metadata<'a> {
    /// Last operation wins, matching kitty's duplicate-key handling.
    pub(super) kind: &'a [u8],
    /// Sanitized ID is bounded like the reference terminal's 512-byte multiplexer ID.
    pub(super) id: String,
    /// Unknown locations mean standard clipboard, as the protocol specifies.
    pub(super) primary: bool,
    /// Decoded MIME is UTF-8, bounded, and contains no separators or controls.
    pub(super) mime: String,
    /// Passwords without a human-readable name have no protocol meaning.
    pub(super) password: Option<crate::clipboard::Secret>,
}
/// Parses syntactically complete records and validates encoded textual metadata.
pub(super) fn parse(raw: &[u8]) -> Option<Metadata<'_>> {
    let mut kind = None;
    let mut id = b"".as_slice();
    let mut loc = b"".as_slice();
    let mut mime = b"".as_slice();
    let mut name = b"".as_slice();
    let mut pw = b"".as_slice();
    for record in raw.split(|b| *b == b':') {
        let equals = record.iter().position(|b| *b == b'=')?;
        let (key, value) = (&record[..equals], &record[equals + 1..]);
        match key {
            b"type" => kind = Some(value),
            b"id" => id = value,
            b"loc" => loc = value,
            b"mime" => mime = value,
            b"name" => name = value,
            b"pw" => pw = value,
            _ => {}
        }
    }
    // A syntactically valid credential grants nothing until Session validates it.
    let name = base64::decode(name, 256)?;
    let password = base64::decode(pw, 128)?;
    let mime = base64::decode(mime, 256)?;
    if !mime.is_empty() && !valid_mime(&mime) {
        return None;
    }
    Some(Metadata {
        kind: kind?,
        id: id
            .iter()
            .copied()
            .filter(|b| b.is_ascii_alphanumeric() || b"-_+.".contains(b))
            .take(512)
            .map(char::from)
            .collect(),
        primary: loc == b"primary",
        mime,
        password: (!name.is_empty() && !password.is_empty())
            .then_some(crate::clipboard::Secret(password)),
    })
}
/// Whitespace/control-free names cannot inject fields into a listing or host label.
pub(super) fn valid_mime(text: &str) -> bool {
    !text.is_empty() && text.len() <= 256 && text.bytes().all(|b| b.is_ascii_graphic())
}

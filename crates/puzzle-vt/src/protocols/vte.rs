//! vte protocol interpretation; common state stays in Terminal (D-58).
//! Reference: <https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda>.
//! OSC 8 selects an interned link for future cells; shared policy controls opening.
use super::strings::payload;
use crate::terminal::Terminal;

/// Starts or closes a link span while enforcing the 2 KiB URI limit.
pub(crate) fn hyperlink(term: &mut Terminal, params: &[&[u8]]) {
    // A closing span affects future printing, not past cell metadata.
    // Link interning belongs to the shared table; this handler only interprets OSC 8.
    let uri = payload(params, 2);
    if uri.is_empty() {
        term.current_link = None;
        return;
    }
    if uri.len() > 2048 {
        // Reject before interning so hostile URIs cannot occupy a table slot.
        tracing::warn!(
            event = "vt.link_too_long",
            length = uri.len(),
            "discarded OSC 8 URI"
        );
        return;
    }
    // The optional id groups the same hyperlink across separated cells;
    // other OSC 8 parameter keys are ignored but do not corrupt the URI.
    let group = params
        .get(1)
        .and_then(|raw| std::str::from_utf8(raw).ok())
        .and_then(|text| text.split(':').find_map(|part| part.strip_prefix("id=")));
    term.current_link = term.links.insert(&uri, group).map(|id| id.0);
}

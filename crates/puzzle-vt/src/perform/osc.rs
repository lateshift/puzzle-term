//! Bounded OSC dispatch after vte's 64 KiB fixed buffer (§6.4, §6.5).
//!
//! OSC carries untrusted application-supplied strings, unlike CSI's small
//! numeric parameters. vte discards bytes past the const-generic cap; an
//! OSC 8 or 52 whose visible payload reaches that cap is rejected instead
//! of accepting a shortened URI or clipboard value (§16). Each handler
//! owns one kind of state: titles, colors, links or host-facing requests.
//! Replies retain the caller's BEL or ST terminator exactly (§6.12).
//! OSC 5522 additionally checks original raw controls before accepting base64.
//! Its request event defers permission and acknowledgement to the Session owner.
//! OSC 66 also uses raw capture: control removal must not turn rejected text into
//! an accepted rectangle, nor may the parser's separator limit truncate a run.

/// Small strict decoder for OSC 52's base64 text.
mod base64;
/// Palette and default-color changes and query replies.
mod color;
/// Common xterm pointer-shape requests.
mod host;
/// Common xterm OSC 52 clipboard requests.
mod link;
/// Window and icon titles plus the bounded title stack.
mod title;

use crate::terminal::Terminal;

/// Dispatches a parsed OSC command, rejecting incomplete sensitive payloads.
pub(super) fn dispatch(term: &mut Terminal, params: &[&[u8]], bell: bool) {
    let Some(code) = params
        .first()
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .and_then(|text| text.parse::<u16>().ok())
    else {
        return;
    };
    // vte's raw buffer excludes separators, so sum the exposed slices.
    // OSC 8/52 need only three fields; sixteen means later text was lost.
    let truncated = params.iter().map(|part| part.len()).sum::<usize>()
        >= crate::osc::MAX_BUFFER_BYTES
        || params.len() >= 16;
    if truncated && matches!(code, 8 | 52) {
        tracing::warn!(
            event = "vt.osc_truncated",
            code,
            "discarded truncated OSC payload"
        );
        return;
    }
    dispatch_code(term, code, params, bell, truncated);
}

/// Routes validated numbers to common-base or defining-origin handlers.
fn dispatch_code(term: &mut Terminal, code: u16, params: &[&[u8]], bell: bool, truncated: bool) {
    // Keep the common xterm family here; extensions route to their defining projects.
    // OSC 9 is shared on the wire: its 4 subtype belongs to ConEmu, not iTerm2.
    match code {
        0..=2 => title::dispatch(term, code, params),
        4 | 5 | 10..=19 | 104 | 105 | 110..=119 => color::dispatch(term, code, params, bell),
        7 => crate::protocols::apple::cwd(term, params),
        8 => crate::protocols::vte::hyperlink(term, params),
        9 if params.get(1) == Some(&b"4".as_slice()) => {
            crate::protocols::conemu::progress(term, params);
        }
        9 => crate::protocols::iterm2::notification(term, params),
        21 => crate::protocols::kitty::colors::dispatch(term, params, bell),
        22 => host::pointer(term, params),
        52 => link::clipboard(term, params),
        66 => {
            // vte may drop controls or later semicolons; use the exact observed payload.
            if let Some(raw) = term.extended_osc_wire() {
                crate::protocols::kitty::text_sizing::dispatch(term, &raw);
            }
        }
        72 => crate::protocols::kitty::dnd::dispatch(term, params, bell),
        99 => {
            // Raw capture preserves semicolons beyond vte's parameter limit and
            // retains controls for strict encoded-data validation.
            if let Some(raw) = term.extended_osc_wire() {
                // Only the first two separators are structural; later ones are title/body text.
                let fields: Vec<_> = raw.splitn(3, |b| *b == b';').collect();
                crate::protocols::kitty::notifications::dispatch(
                    term,
                    &fields,
                    raw.len() >= crate::osc::MAX_BUFFER_BYTES,
                );
            }
        }
        3008 => {
            // Raw metadata retains controls for per-field validation and all fifteen keys.
            // Invalid optional values cannot hide a valid context ID or ancestor end.
            if let Some(raw) = term.extended_osc_wire() {
                crate::protocols::uapi::dispatch(term, &raw);
            }
        }
        5522 => crate::protocols::kitty::clipboard::dispatch(
            term,
            params,
            truncated || term.clipboard_wire_invalid(),
        ),
        133 | 633 => crate::protocols::finalterm::shell_mark(term, params),
        777 => crate::protocols::rxvt::notification_777(term, params),
        _ => {}
    }
}

use crate::protocols::strings::payload;

/// Terminator bytes used by color-query replies and no other OSC writes.
pub(super) fn terminator(bell: bool) -> &'static [u8] {
    if bell { b"\x07" } else { b"\x1b\\" }
}

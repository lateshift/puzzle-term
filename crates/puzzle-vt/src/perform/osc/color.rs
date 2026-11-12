//! Xterm colour operations over the shared table (OSC 4/5/10–19 and resets).
//! Reference: https://invisible-island.net/xterm/ctlseqs/ctlseqs.html
//! Each pair is independent; malformed values never discard later valid pairs.
//! Palette aliases 256–260 and OSC 5 address the very same special entries.
//!
//! OSC 4/5 carry index/spec pairs; a bad pair cannot hide a later valid pair.
//! OSC 10–19 carry successive specifications starting at the given role. This
//! permits one request to set foreground and query background/cursor in order.
//! Reset numbers add 100 to the corresponding set/query number. Palette and
//! special resets accept an index list, or reset their entire group when empty.
//! Dynamic resets affect exactly one role and reveal its current theme default.
//! Every set uses the common colour decoder; no wire family owns a name table.
//! Changes mark paint dirty, leaving cell colours tagged so old text recolours.
//! This handler does no I/O; replies append to Terminal's ordered output buffer.
//! The session owner flushes them with other replies after its parser slice.
//! Queries are nonidentifying: only palette and theme RGB values are disclosed.
//! Xterm reports are RGB-only, so undefined roles resolve to the default ink;
//! kitty's empty-value report is implemented separately over the same state.
//! Query formatting replicates bytes to 16-bit channels, preserving endpoints.
//! Labels are constructed only for replies, avoiding allocation on colour sets.
use crate::{
    color_spec,
    color_table::{ColorSlot, ColorTable, ColorValue, DynamicColor, SpecialColor},
    terminal::Terminal,
};
use std::fmt::Write;

/// Applies sets/queries/resets while preserving the request's BEL/ST terminator.
pub(super) fn dispatch(term: &mut Terminal, code: u16, params: &[&[u8]], bell: bool) {
    // The common OSC dispatcher routes only the colour numbers listed here.
    match code {
        4 | 5 => indexed(term, code, params, bell),
        10..=19 => {
            // Xterm accepts successive dynamic specifications starting at this code.
            for (offset, spec) in params.iter().skip(1).enumerate() {
                let number = usize::from(code) + offset;
                // Stop at OSC 19: additional fields cannot alias kitty-only entries.
                let Some(role) = DynamicColor::from_osc(number as u16) else {
                    break;
                };
                operate(
                    term,
                    ColorSlot::Dynamic(role),
                    spec,
                    number as u16,
                    None,
                    bell,
                );
            }
        }
        104 | 105 => {
            // The conventional empty first parameter requests an all-group reset.
            if params.len() == 1 || params.get(1) == Some(&b"".as_slice()) {
                term.colors.reset_group(code == 105);
            } else {
                // Reset each valid index independently; an unknown index is harmless.
                for bytes in params.iter().skip(1) {
                    if let Some(slot) = index(bytes, code == 105) {
                        term.colors.reset(slot);
                    }
                }
            }
            // Even stored-only values travel consistently with a later publication.
            term.screen_mut().damage.full = true;
        }
        110..=119 => {
            if let Some(role) = DynamicColor::from_osc(code - 100) {
                term.colors.reset(ColorSlot::Dynamic(role));
                term.screen_mut().damage.full = true;
            }
        }
        _ => {}
    }
}

/// Processes complete index/spec pairs; a dangling index has no effect.
fn indexed(term: &mut Terminal, code: u16, params: &[&[u8]], bell: bool) {
    // chunks_exact deliberately ignores a dangling index without a colour spec.
    for pair in params.get(1..).unwrap_or(&[]).chunks_exact(2) {
        // Range-check before touching the table, rather than truncating to a byte.
        let Some(slot) = index(pair[0], code == 5) else {
            continue;
        };
        // Validated decimal addresses cannot inject controls into a reply.
        operate(term, slot, pair[1], code, Some(pair[0]), bell);
    }
}

/// Converts only in-range decimal indexes; malformed input cannot wrap.
fn index(bytes: &[u8], special: bool) -> Option<ColorSlot> {
    // Non-UTF8, negative, overflowing and out-of-range addresses are rejected.
    let index = std::str::from_utf8(bytes).ok()?.parse::<u16>().ok()?;
    if special {
        SpecialColor::from_index(index).map(ColorSlot::Special)
    } else {
        ColorTable::indexed(index)
    }
}

/// Shares the allocation-free colour grammar across all xterm colour numbers.
fn operate(
    term: &mut Terminal,
    slot: ColorSlot,
    spec: &[u8],
    code: u16,
    index: Option<&[u8]>,
    bell: bool,
) {
    if spec == b"?" {
        // Xterm cannot encode undefined colours; resolve them to ordinary ink.
        let rgb = term.colors.get(slot).rgb().unwrap_or_else(|| {
            term.colors
                .get(ColorSlot::Dynamic(DynamicColor::Foreground))
                .rgb()
                .unwrap_or([229; 3])
        });
        // Decimal framing is allocated only for queries, never for colour sets.
        // Validated decimal indexes are the only external bytes copied into framing.
        let mut reply = format!("\x1b]{code};");
        if let Some(index) = index {
            reply.push_str(std::str::from_utf8(index).unwrap_or(""));
            reply.push(';');
        }
        // Append the same terminator form used by the incoming request.
        append_rgb(&mut reply, rgb);
        term.replies.extend_from_slice(reply.as_bytes());
        term.replies.extend_from_slice(super::terminator(bell));
    } else if let Some(rgb) = color_spec::parse(spec) {
        // A malformed spec never behaves like reset or a transparent black value.
        term.colors.set(slot, ColorValue::Rgb(rgb));
        term.screen_mut().damage.full = true;
    }
}

/// Encodes RGB channels in xterm's 16-bit report form without a temporary string.
fn append_rgb(reply: &mut String, rgb: [u8; 3]) {
    // Byte replication preserves 8-bit endpoints in 16-bit query reports.
    let _ = write!(
        reply,
        "rgb:{:04x}/{:04x}/{:04x}",
        u16::from(rgb[0]) * 257,
        u16::from(rgb[1]) * 257,
        u16::from(rgb[2]) * 257
    );
}

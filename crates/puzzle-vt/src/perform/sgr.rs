//! SGR rendition parser for xterm's semicolon and colon forms (§6.5).
//!
//! The pen is a template for new cells. Existing cells keep their tagged
//! colors when the pen changes, so a palette override can recolor them at
//! render time. SGR reset does not close an OSC 8 hyperlink or remove the
//! independent DECSCA protection attribute. Underline color is sparse:
//! adding four bytes to every 16-byte cell for a rare style would violate
//! the grid's fixed memory budget. vte limits parameter storage to 32
//! subparameters, so this parser needs no heap-backed scratch list.
//!
//! SGR is a sequence of independent operations, applied left to right.
//! `0` clears graphic rendition, while `22` clears bold and faint together.
//! Underline styles use colon subparameters because semicolons separate
//! distinct SGR operations; `4:3` means curly underline, whereas `4;3`
//! means underline plus italic. The style occupies three bits in `Attrs`.
//!
//! Color 38/48/58 select foreground/background/underline independently.
//! Mode 5 takes one palette index and mode 2 takes RGB components. xterm
//! also accepts a color-space slot in the colon form; the supported RGB
//! path ignores that slot while retaining the three components. Missing
//! components leave the previous color unchanged, avoiding accidental
//! state changes from malformed application output.
//!
//! Palette colors remain tagged indexes in cells. That is necessary for
//! OSC 4 changes to recolor old text without rewriting the grid (§6.2).
//! Underline color cannot fit the sixteen-byte cell, so it is copied to a
//! sparse row extra only when a printed cell actually uses one.

use crate::{
    cell::{Attrs, Cell, CellFlags, PackedColor},
    terminal::Terminal,
};
use vte::Params;

/// Applies every SGR code in order, preserving unrelated pen attributes.
pub(super) fn dispatch(term: &mut Terminal, params: &Params) {
    // vte preserves each colon group as one slice but yields semicolon
    // groups separately, so extended colors need both parsing paths.
    let mut parts = params.iter();
    while let Some(part) = parts.next() {
        let code = part.first().copied().unwrap_or(0);
        match code {
            38 | 48 | 58 => {
                // Extended colors consume following semicolon parameters or
                // the subparameters already present in a colon-form slice.
                if let Some(color) = parse_color(part, &mut parts) {
                    match code {
                        38 => term.pen.fg = color,
                        48 => term.pen.bg = color,
                        _ => term.underline_color = Some(color),
                    }
                }
            }
            59 => term.underline_color = None,
            // SGR 59 resets just underline color, not underline style.
            _ => basic(term, part),
        }
    }
}

/// Changes one non-extended SGR attribute or indexed ANSI color.
fn basic(term: &mut Terminal, part: &[u16]) {
    // An omitted code is ECMA-48's reset operation.
    let code = part.first().copied().unwrap_or(0);
    let attrs = &mut term.pen.attrs;
    match code {
        0 => {
            // DECSCA protection is not an SGR attribute (xterm ctlseqs).
            let protected = term.pen.flags & CellFlags::PROTECTED;
            term.pen = Cell::erased(PackedColor::DEFAULT);
            term.pen.flags = protected;
            term.underline_color = None;
        }
        1 => attrs.insert(Attrs::BOLD),
        2 => attrs.insert(Attrs::DIM),
        3 => attrs.insert(Attrs::ITALIC),
        4 => attrs.set_underline_style(part.get(1).copied().unwrap_or(1).min(5) as u8),
        // The `4:n` subparameter is the only branch that uses a second
        // value without consuming a new semicolon-separated operation.
        5 | 6 => attrs.insert(Attrs::BLINK),
        7 => attrs.insert(Attrs::INVERSE),
        8 => attrs.insert(Attrs::HIDDEN),
        9 => attrs.insert(Attrs::STRIKE),
        21 => attrs.set_underline_style(2),
        // xterm ctlseqs retains the old SGR 21 double-underline alias.
        22 => attrs.remove(Attrs::BOLD | Attrs::DIM),
        // Normal intensity cancels both bold and faint, not italic.
        23 => attrs.remove(Attrs::ITALIC),
        24 => attrs.set_underline_style(0),
        25 => attrs.remove(Attrs::BLINK),
        27 => attrs.remove(Attrs::INVERSE),
        28 => attrs.remove(Attrs::HIDDEN),
        29 => attrs.remove(Attrs::STRIKE),
        53 => attrs.insert(Attrs::OVERLINE),
        55 => attrs.remove(Attrs::OVERLINE),
        30..=37 => term.pen.fg = PackedColor::palette((code - 30) as u8),
        // Bright ANSI slots are the second half of the base 16 colors.
        90..=97 => term.pen.fg = PackedColor::palette((code - 90 + 8) as u8),
        39 => term.pen.fg = PackedColor::DEFAULT,
        40..=47 => term.pen.bg = PackedColor::palette((code - 40) as u8),
        100..=107 => term.pen.bg = PackedColor::palette((code - 100 + 8) as u8),
        49 => term.pen.bg = PackedColor::DEFAULT,
        _ => {}
    }
}

/// Reads indexed or RGB values without allocating for nested CSI syntax.
fn parse_color<'a>(
    part: &[u16],
    rest: &mut impl Iterator<Item = &'a [u16]>,
) -> Option<PackedColor> {
    if part.len() > 1 {
        // A colon form contains its mode and components inside this part.
        return match part[1] {
            5 => part
                // Palette indexes outside 0–255 are clamped, not wrapped.
                .get(2)
                .map(|index| PackedColor::palette((*index).min(255) as u8)),
            2 => {
                // `38:2::r:g:b` has an empty color-space ID at index 2;
                // `38:2:r:g:b` omits it. Both produce the same RGB bytes.
                let start = if part.len() >= 6 { 3 } else { 2 };
                rgb_from(part.get(start..start + 3)?)
            }
            _ => None,
        };
    }
    let mode = rest.next()?.first().copied()?;
    // The semicolon form consumes following SGR parameters in order.
    match mode {
        5 => Some(PackedColor::palette(
            rest.next()?.first().copied()?.min(255) as u8,
        )),
        2 => {
            let red = rest.next()?.first().copied()?;
            let green = rest.next()?.first().copied()?;
            let blue = rest.next()?.first().copied()?;
            rgb_from(&[red, green, blue])
        }
        _ => None,
    }
}

/// Clamps CSI components to bytes before packing them into a cell.
fn rgb_from(parts: &[u16]) -> Option<PackedColor> {
    // CSI parameters are u16; packed color channels are eight bits.
    Some(PackedColor::rgb(
        parts.first().copied()?.min(255) as u8,
        parts.get(1).copied()?.min(255) as u8,
        parts.get(2).copied()?.min(255) as u8,
    ))
}

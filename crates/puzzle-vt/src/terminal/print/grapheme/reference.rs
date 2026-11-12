//! Pre-M16.5 candidate-string path, retained only for differential tests.
//! The boundary decision and cap are copied unchanged from the accepted code;
//! the width/storage methods remain shared so this exercises identical cells.
use super::{Terminal, cluster_base};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

impl Terminal {
    /// Extends one prior Unicode grapheme while retaining its cell origin.
    pub(super) fn extend_grapheme_reference<const WATCH: bool>(&mut self, next: char) -> bool {
        let row_index = self.cursor.row;
        // Pending wrap still points at the glyph just printed in the final
        // column. Otherwise the cursor points one column after that glyph.
        let column = if self.cursor.pending_wrap {
            self.cursor.column
        } else if self.cursor.column > 0 {
            self.cursor.column - 1
        } else {
            return false;
        };
        let Some(row) = self.screen().row(row_index) else {
            return false;
        };
        let Some((column, cell)) = cluster_base(row, column) else {
            return false;
        };
        // A wide spacer is never a glyph origin. `cluster_base` walks back
        // to its leading cell before the boundary decision is made.
        // An existing sparse cluster is the authoritative text. A plain
        // leading cell contributes its scalar as the first cluster member.
        let mut text = row
            .extras
            .iter()
            .find(|extra| extra.column == column)
            .and_then(|extra| extra.cluster.clone())
            .unwrap_or_else(|| char::from_u32(cell.ch).unwrap_or('\u{fffd}').to_string());
        text.push(next);
        // UAX #29 determines whether this scalar is in the same display
        // unit. Width alone cannot decide it: two wide emoji may be joined
        // by ZWJ, while two independent narrow letters must stay separate.
        if UnicodeSegmentation::graphemes(text.as_str(), true).count() != 1 {
            return false;
        }
        if text.len() > 32 {
            // A hostile run may not consume unbounded sparse metadata (§16).
            // Consuming the scalar still preserves the physical cell grid.
            return true;
        }
        let width = grapheme_width(&text);
        self.store_grapheme::<WATCH>(row_index, column, cell, text, width);
        true
    }
}

/// Resolves selectors and keycap presentation over the full cluster.
fn grapheme_width(text: &str) -> usize {
    // A keycap is wide only when its base is followed by VS16 before the
    // enclosing mark; a plain digit plus the mark remains one cell.
    let keycap = text.ends_with('\u{20e3}')
        && matches!(text.chars().next(), Some('#' | '*' | '0'..='9'))
        && text.chars().nth(1) == Some('\u{fe0f}');
    if keycap {
        2
    } else {
        // The most recent text/emoji selector controls presentation.
        // In particular VS15 narrows watch symbols that start wide.
        let mut base = None;
        let mut presentation = None;
        let mut filtered = String::with_capacity(text.len());
        // Keep a separate filtered view for unicode-width. The authoritative
        // sparse cluster retains valid selectors for shaping and rendering.
        for scalar in text.chars() {
            let selector = matches!(scalar, '\u{fe0e}' | '\u{fe0f}');
            let valid = base.is_some_and(is_extended_pictographic);
            if selector && valid {
                // Both selectors refer to the most recent base; the final
                // one wins even if another selector sits between them.
                presentation = Some(scalar);
            }
            if !selector || valid {
                // unicode-width treats some invalid VS16 sequences as emoji.
                // The reference terminal discards those selectors before width calculation.
                filtered.push(scalar);
            }
            if !selector {
                base = Some(scalar);
            }
        }
        match presentation {
            Some('\u{fe0e}') => 1,
            Some('\u{fe0f}') => 2,
            _ => UnicodeWidthStr::width(filtered.as_str()).clamp(1, 2),
        }
    }
}

/// Uses UAX #29's GB11 boundary to query Extended_Pictographic without a table.
fn is_extended_pictographic(ch: char) -> bool {
    // UAX #29's GB11 joins an EP scalar after `EP ZWJ`. The segmentation
    // library exposes that boundary even though it does not expose an EP
    // property query. Extend scalars also join under GB9, so reject their
    // zero width before applying this probe.
    // GB9 also joins Extend scalars to the probe's trailing ZWJ, so a
    // zero-width selector must be excluded before using GB11 as an EP test.
    if ch.is_ascii() || UnicodeWidthChar::width(ch) == Some(0) {
        return false;
    }
    let mut bytes = [0_u8; 12];
    let prefix = "👩\u{200d}".as_bytes();
    bytes[..prefix.len()].copy_from_slice(prefix);
    let encoded = ch.encode_utf8(&mut bytes[prefix.len()..]);
    let length = prefix.len() + encoded.len();
    std::str::from_utf8(&bytes[..length])
        .is_ok_and(|text| UnicodeSegmentation::graphemes(text, true).count() == 1)
}

//! D-31 selector widths, matching the VT's reference-parity table (§6.7).
//!
//! unicode-width alone chooses a different repeated-selector result. Apply
//! the measured last-valid-selector policy before assigning terminal cells.
//! A small inline UTF-8 buffer avoids allocations for bounded VT clusters.
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Counts cells for one grapheme, without changing its retained source bytes.
/// The VT stores both ordinary scalars and multi-scalar clusters; matching
/// those two insertion paths is necessary for a trailing flag candidate.
/// Selector filtering changes only the width calculation, never shaping.
/// Inline storage covers every 32-byte cluster admitted by the VT bound.
pub(crate) fn cluster_width(text: &str) -> usize {
    // Plain combining marks still attach without advancing a grid cell.
    if !text.contains(['\u{fe0e}', '\u{fe0f}']) {
        // The VT starts an unpaired regional indicator as a wide flag
        // candidate, and its partner retains the same two-cell allocation.
        if text
            .chars()
            .next()
            .is_some_and(|ch| ('\u{1f1e6}'..='\u{1f1ff}').contains(&ch))
        {
            return 2;
        }
        return UnicodeWidthStr::width(text).min(2);
    }
    // The reference terminal's measured keycap path accepts VS16 before the enclosing mark.
    if text.ends_with('\u{20e3}')
        && matches!(text.chars().next(), Some('#' | '*' | '0'..='9'))
        && text.chars().nth(1) == Some('\u{fe0f}')
    {
        return 2;
    }
    let mut base = None;
    let mut selector = None;
    let mut filtered = smallvec::SmallVec::<[u8; 32]>::new();
    for scalar in text.chars() {
        // A selector after a non-pictographic base is invisible and cannot
        // widen x or a combining mark, even if unicode-width would do so.
        let selecting = matches!(scalar, '\u{fe0e}' | '\u{fe0f}');
        let valid = base.is_some_and(extended_pictographic);
        if selecting && valid {
            selector = Some(scalar);
        }
        if !selecting || valid {
            let mut bytes = [0; 4];
            filtered.extend_from_slice(scalar.encode_utf8(&mut bytes).as_bytes());
        }
        if !selecting {
            base = Some(scalar);
        }
    }
    match selector {
        Some('\u{fe0e}') => 1,
        Some('\u{fe0f}') => 2,
        // Every appended byte came from encode_utf8; a failed conversion is
        // nonetheless treated as empty input instead of a panic (R8).
        _ => std::str::from_utf8(&filtered).map_or(0, UnicodeWidthStr::width),
    }
}

/// Queries GB11's Extended_Pictographic boundary through the approved segmenter.
fn extended_pictographic(ch: char) -> bool {
    // GB9 also joins Extend; exclude zero-width characters before probing GB11.
    if ch.is_ascii() || UnicodeWidthChar::width(ch) == Some(0) {
        return false;
    }
    let mut bytes = [0_u8; 12];
    let prefix = "👩\u{200d}".as_bytes();
    bytes[..prefix.len()].copy_from_slice(prefix);
    let encoded = ch.encode_utf8(&mut bytes[prefix.len()..]);
    let length = prefix.len() + encoded.len();
    // An EP after EP+ZWJ forms one grapheme. No new Unicode property crate
    // or downloaded table is needed to test the segmentation rule itself.
    std::str::from_utf8(&bytes[..length]).is_ok_and(|text| text.graphemes(true).count() == 1)
}

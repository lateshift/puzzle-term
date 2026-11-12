//! Unicode clusters, fallback faces and source-cell ownership (§8.3–§8.4).
//!
//! Harfrust consumes runs of UTF-8, but the terminal advances in grid
//! columns. This stage records both byte boundaries and Unicode cell widths
//! before shaping can replace several source characters with a ligature.
//! It also chooses one face for every scalar in a grapheme; a missing mark
//! must not quietly turn into a `.notdef` in the primary font.
//!
//! Private-use symbols may paint into a following blank cell. That grant
//! belongs to source layout rather than the renderer: shaping may discard
//! the blank glyph, but the renderer still needs to know the safe ink span.
use crate::presentation::width::cluster_width;
use crate::{
    builtin::{BUILTIN_FACE, BuiltinCell, TOFU_FACE},
    font_set::{FontSet, FontStyle, is_default_ignorable},
};
use unicode_segmentation::UnicodeSegmentation;

/// Source cluster with its selected face and first terminal column.
pub(super) struct Cluster {
    /// UTF-8 start within the caller's text.
    pub(super) byte: usize,
    /// Exclusive UTF-8 end, used to select complete shaping runs.
    pub(super) end: usize,
    /// Font face covering every scalar in this grapheme.
    pub(super) face: u32,
    /// First terminal column, independent of font advances.
    pub(super) cell: u32,
    /// Number of grid columns assigned by Unicode width.
    pub(super) span: u32,
    /// Whether a PUA scalar may paint into the next blank cell.
    pub(super) overflow: bool,
}

/// Resolves complete graphemes before any script-dependent shaping call.
pub(super) fn collect_into(
    fonts: &mut FontSet,
    text: &str,
    style: FontStyle,
    symbol_overflow: bool,
    builtin_glyphs: bool,
    clusters: &mut Vec<Cluster>,
    preferred: Option<u32>,
) -> Result<(), String> {
    // Retaining capacity lets consecutive dirty rows share one grapheme
    // buffer; the final placements own none of these source records (P1).
    clusters.clear();
    let mut cell = 0_u32;
    for (byte, grapheme) in text.grapheme_indices(true) {
        // Standalone selectors and joiners are malformed input, not ink.
        // They consume no cell and must not reach procedural tofu fallback.
        if grapheme.chars().all(is_default_ignorable) {
            continue;
        }
        // A single box, block or Braille scalar uses device-cell geometry
        // even when the primary font advertises an approximate glyph.
        // Multi-scalar graphemes remain with font shaping and fallback.
        let face = choose_face(fonts, grapheme, style, builtin_glyphs, preferred)?;
        // A zero-width cluster is allowed because marks attach to a
        // preceding cell, while ordinary wide scalars claim two columns.
        let span =
            u32::try_from(cluster_width(grapheme)).map_err(|_| "cluster width exceeds grid")?;
        // Harfrust reports UTF-8 byte offsets. Retaining both bounds lets
        // its output return to the correct original terminal cell.
        clusters.push(Cluster {
            byte,
            end: byte + grapheme.len(),
            face,
            cell,
            span,
            overflow: false,
        });
        // An absurd external line is an error rather than a wrapped cell
        // coordinate that could paint over an earlier glyph.
        cell = cell.checked_add(span).ok_or("text cell count overflow")?;
    }
    overflow(text, symbol_overflow, clusters);
    Ok(())
}

/// Recognizes the Unicode private-use ranges used by Nerd Font icons.
fn is_pua(ch: char) -> bool {
    matches!(ch as u32, 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD)
}

/// Sums Unicode grapheme widths for a target-size or grid-bound check.
pub(super) fn cell_count(text: &str) -> Result<u32, String> {
    text.graphemes(true).try_fold(0_u32, |cells, cluster| {
        let width =
            u32::try_from(cluster_width(cluster)).map_err(|_| "cluster width exceeds grid")?;
        cells
            .checked_add(width)
            .ok_or("text cell count overflow".into())
    })
}

/// Selects procedural ink or one face covering the complete caller-owned cluster.
pub(super) fn choose_face(
    fonts: &mut FontSet,
    grapheme: &str,
    style: FontStyle,
    builtin_glyphs: bool,
    preferred: Option<u32>,
) -> Result<u32, String> {
    let face = if builtin_glyphs
        && grapheme.chars().count() == 1
        && grapheme.chars().next().is_some_and(BuiltinCell::supports)
    {
        BUILTIN_FACE
    } else {
        // A face must cover every scalar; absent clusters reach a
        // readable builtin codepoint box without aborting the row.
        fonts
            .choose_face_from(grapheme, style, preferred)?
            .unwrap_or_else(|| {
                // No-face selectors keep their API result, but a stored grid blank
                // needs a run identity to retain its span without procedural tofu.
                if crate::font_set::is_blank_cluster(grapheme) {
                    fonts.primary_face(style)
                } else {
                    TOFU_FACE
                }
            })
    };
    Ok(face)
}

/// Grants a private-use symbol room in the next explicitly blank cell.
pub(super) fn overflow(text: &str, symbol_overflow: bool, clusters: &mut [Cluster]) {
    // Resolve overflow before shaping can remove the following space.
    if symbol_overflow {
        for index in 0..clusters.len() {
            let current = &text[clusters[index].byte..clusters[index].end];
            let pua = current.chars().count() == 1 && current.chars().next().is_some_and(is_pua);
            let blank_next = clusters
                .get(index + 1)
                .is_none_or(|next| text[next.byte..next.end].chars().all(char::is_whitespace));
            clusters[index].overflow = pua && blank_next;
        }
    }
}

//! Resolved HTML rendition and escaping for buffer export and clipboard (§6.13).
//!
//! A run ends at a hard line boundary or a visible style change. Comparing resolved
//! CSS merges equivalent palette/RGB cells and ignores nonvisual cell flags.
//! HTML is a cold export: only the resulting string and style scratch allocate.
//! Inversion precedes faint exactly as in §10.5; hidden text stays selectable.
//!
//! HTML describes the underlying cells, without transient selection colors.
//! Application reverse video is retained because it changes cell rendition.
//! The destination's CSS system colors are used only when VT has no host theme.
//! Clipboard and whole-buffer exports therefore share the same color ordering.

use super::*;
use crate::color_table::{ColorSlot, ColorValue, DynamicColor};

/// Streaming cell writer with one open span and reusable CSS scratch.
#[derive(Default)]
pub(super) struct Runs {
    /// CSS of the open span, empty when no span is open.
    /// Empty is an unambiguous sentinel: every real style starts with color.
    previous: String,
    /// Resolved style of the next cell; reused between cells.
    /// It does not include text, coordinates, link IDs or wide-spacer flags.
    scratch: String,
}

/// Both exports pass their borrowed cells through this writer.
impl Runs {
    /// Appends one lead cell, merging it with an identical resolved style.
    pub(super) fn cell(&mut self, output: &mut String, cell: CellView<'_>, term: &Terminal) {
        // A spacer is the second half of an already emitted wide cluster.
        if cell.width == 0 {
            return;
        }
        // Palette aliases are indistinguishable after CSS resolution; merging
        // their text is safe even though their packed cell tags differ.
        style(&mut self.scratch, cell, term);
        if self.previous != self.scratch {
            // Close before opening so a later style cannot nest inside the old one.
            self.finish(output);
            output.push_str("<span style=\"");
            output.push_str(&self.scratch);
            output.push_str("\">");
            // Save style identity independently of the scratch overwritten next cell.
            self.previous.clone_from(&self.scratch);
        }
        // Escaping is independent of merging; every cell passes through it once.
        escaped(output, cell.text);
    }

    /// Closes the open run before a hard line boundary or closing pre tag.
    pub(super) fn finish(&mut self, output: &mut String) {
        if !self.previous.is_empty() {
            output.push_str("</span>");
            // Retain capacity across physical rows in a large exported document.
            self.previous.clear();
        }
    }
}

/// Resolves packed colors without freezing unknown host defaults to invented RGB.
/// This legacy export leaves unoverridden colours to the destination's CSS.
/// The table supplies application overrides without changing that export contract.
fn rgb(color: PackedColor, term: &Terminal, foreground: bool) -> Option<[u8; 3]> {
    // Unlike a render publication, this export has no explicit host-theme input.
    // Resolving standalone VT defaults here would hard-code its dark appearance.
    // Both xterm and kitty overrides still take precedence over CSS defaults.
    match color.0 >> 24 {
        2 => Some([(color.0 >> 16) as u8, (color.0 >> 8) as u8, color.0 as u8]),
        // Indexed tags name the current palette, including application OSC 4 changes.
        1 => term
            .colors
            .override_value(ColorSlot::Palette(color.0 as u8))
            .and_then(ColorValue::rgb),
        // OSC 10 and 11 can change these defaults independently of the stored cells.
        _ if foreground => term
            .colors
            .override_value(ColorSlot::Dynamic(DynamicColor::Foreground))
            .and_then(ColorValue::rgb),
        _ => term
            .colors
            .override_value(ColorSlot::Dynamic(DynamicColor::Background))
            .and_then(ColorValue::rgb),
    }
}

/// Appends a known RGB token or the CSS system default of its original channel.
fn color(output: &mut String, value: Option<[u8; 3]>, foreground: bool) {
    use std::fmt::Write;
    if let Some([r, g, b]) = value {
        // Writing into the export avoids a separate allocation for each RGB token.
        let _ = write!(output, "#{r:02x}{g:02x}{b:02x}");
    } else {
        output.push_str(if foreground { "CanvasText" } else { "Canvas" });
    }
}

/// Converts visual attributes to one stable CSS style, independent of raw tags.
fn style(output: &mut String, cell: CellView<'_>, term: &Terminal) {
    output.clear();
    let attrs = cell.raw.attrs;
    let mut fg = rgb(cell.raw.fg, term, true);
    let mut bg = rgb(cell.raw.bg, term, false);
    // Screen reverse video and per-cell inverse cancel when both are enabled.
    let inverse = attrs.contains(Attrs::INVERSE) ^ term.modes.get(true, 5).unwrap_or(false);
    // Swap resolved RGB, not packed tags whose default-channel meaning differs.
    if inverse {
        std::mem::swap(&mut fg, &mut bg);
    }
    output.push_str("color:");
    // Faint affects only ink, after the swap. Round as the renderer does.
    if attrs.contains(Attrs::DIM) {
        faint(output, fg, bg, inverse);
    } else {
        color(output, fg, !inverse);
    }
    // Faint never changes the paper, including paper inherited through inversion.
    output.push_str(";background-color:");
    color(output, bg, inverse);
    // Weight and slant form part of resolved run identity as well as its CSS.
    if attrs.contains(Attrs::BOLD) {
        output.push_str(";font-weight:bold");
    }
    if attrs.contains(Attrs::ITALIC) {
        output.push_str(";font-style:italic");
    }
    // Multiple decorations share one CSS property; repeated declarations overwrite.
    decorations(output, attrs);
    if attrs.contains(Attrs::HIDDEN) {
        // Keep a text node for selection/accessibility without rendering its ink.
        output.push_str(";visibility:hidden");
    }
}

/// Mixes known colors numerically, with CSS mixing for unresolved host defaults.
fn faint(output: &mut String, fg: Option<[u8; 3]>, bg: Option<[u8; 3]>, inverse: bool) {
    // Widen before addition so bright RGB pairs cannot overflow u8 arithmetic.
    // Halfway values round upward, matching the renderer's positive f32 round.
    if let (Some(fg), Some(bg)) = (fg, bg) {
        let mixed =
            std::array::from_fn(|i| (u16::from(fg[i]) + u16::from(bg[i])).div_ceil(2) as u8);
        color(output, Some(mixed), true);
    } else {
        // A bare VT buffer has no host theme; system CSS colors stay resolvable.
        output.push_str("color-mix(in srgb,");
        color(output, fg, !inverse);
        output.push_str(" 50%,");
        color(output, bg, inverse);
        output.push(')');
    }
}

/// One declaration preserves simultaneous underline, strike and overline.
fn decorations(output: &mut String, attrs: Attrs) {
    // Underline style zero means absent; other styles all retain the underline line.
    let underline = attrs.underline_style() != 0;
    let strike = attrs.contains(Attrs::STRIKE);
    let overline = attrs.contains(Attrs::OVERLINE);
    // Omitting the property for plain text keeps adjacent plain cells mergeable.
    if !(underline || strike || overline) {
        return;
    }
    output.push_str(";text-decoration:");
    if underline {
        output.push_str("underline");
    }
    if strike {
        // CSS accepts a space-separated set of decoration lines in one declaration.
        if underline {
            output.push(' ');
        }
        output.push_str("line-through");
    }
    if overline {
        if underline || strike {
            output.push(' ');
        }
        output.push_str("overline");
    }
}

/// Escapes every untrusted scalar without copying borrowed cluster strings.
fn escaped(output: &mut String, text: CellText<'_>) {
    // A scalar needs at most four UTF-8 bytes; clusters borrow terminal row extras.
    let mut scalar = [0u8; 4];
    let text = match text {
        CellText::Empty => " ",
        CellText::Scalar(ch) => ch.encode_utf8(&mut scalar),
        CellText::Cluster(text) => text,
    };
    // Text nodes cannot inject tags or entities into the generated spans.
    for ch in text.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            _ => output.push(ch),
        }
    }
}

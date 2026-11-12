//! Retained source identity distinguishes real glyph ink from provisional hex boxes.
//!
//! A nonblank cell alone is insufficient: tofu also has ink. This observer uses
//! the cached neutral requests and current store generations, without preparing
//! a replacement outline or rebuilding the image being diagnosed.
//!
//! Each cached row retains source placements before layer expansion and synthetic
//! weight duplication. Reporting those requests counts a wide cluster exactly
//! once even when its final ink occupies several Slug instances or raster quads.
//! Row-local coordinates survive scrolling and are independent of backend fitting.
//! Vector lookup checks the current atlas generation; complete color graphs also
//! validate enclosing clips and paint handles. Raster lookup uses the original
//! device-size and fit dimensions. None of these lookups can create new ink.
//! The result is an allocated diagnostic list, used outside measured frame work.
use super::{FrameBuilder, context::TextContext};
use puzzle_text::{builtin::TOFU_FACE, raster::RasterKey};

/// A retained neutral request, used only by opt-in presentation diagnostics.
pub struct InkSample {
    /// Logical row identity follows the text while its viewport position changes.
    pub line: puzzle_vt::tracked::LineId,
    /// Source column counts a complete cluster once, before layer expansion.
    pub column: u32,
    /// Viewport row locates the corresponding submitted GPU crop.
    pub row: u32,
    /// Complete cell span includes the spacer of wide glyphs.
    pub span: u32,
    /// Source identity distinguishes replacement text at the same coordinates.
    pub key: puzzle_text::glyph::GlyphKey,
    /// Final store ink is available; this never prepares a missing glyph.
    pub ready: bool,
}

impl FrameBuilder {
    /// Classifies source colors from the exact submitted slot, never a live Session.
    ///
    /// Equal resolved ink and paper cannot be distinguished by an RGB readback.
    /// This diagnostic follows palette/inverse/dim/contrast and the same search
    /// then selection precedence as assembly; missing source cells fail closed.
    pub fn zero_contrast(
        &self,
        snapshot: &crate::frame_snapshot::FrameSnapshot,
        sample: &InkSample,
    ) -> bool {
        let Some(row) = snapshot.retained_row(sample.row as usize) else {
            return false;
        };
        let Some(cell) = crate::frame_snapshot::cell_from_row(row, sample.column as usize) else {
            return false;
        };
        let Some(paint) = snapshot.paint() else {
            return false;
        };
        let (mut ink, mut paper) =
            self.colors
                .resolve(paint, cell.raw.fg, cell.raw.bg, cell.raw.attrs);
        // Query paper replaces application paper without recoloring text.
        let contains = |range: crate::selection::SelectionRange| {
            range
                .columns(row.id, snapshot.columns())
                .is_some_and(|(first, end)| (first..end).contains(&(sample.column as usize)))
        };
        for hit in snapshot.search().iter().filter(|hit| contains(hit.range)) {
            paper = rgb(self.colors.search_background(hit.current));
        }
        // Selected foreground is resolved against selected paper before contrast.
        if snapshot.selection().is_some_and(contains) {
            let (base, _) =
                self.colors
                    .resolve_uncontrasted(paint, cell.raw.fg, cell.raw.bg, cell.raw.attrs);
            ink = rgb(self.colors.selection_ink(paint, super::colors::rgba(base)));
            paper = rgb(self.colors.selection_background(paint));
        }
        ink == paper
    }

    /// Returns viewport row, source column, cell span and resolved-ink flag.
    /// The caller pairs these neutral coordinates with retained GPU cell crops.
    pub fn retained_ink(&self, context: &mut TextContext) -> Vec<[u32; 4]> {
        self.retained_ink_samples(context)
            .into_iter()
            .map(|c| [c.row, c.column, c.span, u32::from(c.ready)])
            .collect()
    }

    /// Reports source identity so observers can distinguish unsupported tofu themselves.
    pub fn retained_ink_samples(&self, context: &mut TextContext) -> Vec<InkSample> {
        let mut cells = Vec::new();
        for (index, row) in self.rows.iter().take(self.visible_rows).enumerate() {
            for source in &row.sources {
                let p = source.placement;
                // Contourless ligature components and blank Braille occupy cells without ink.
                // Only the backend's explicit Empty result qualifies, never a missing handle.
                if source.empty {
                    continue;
                }
                let key = RasterKey {
                    glyph: p.key,
                    size: p.size_px.to_bits(),
                    fit: [p.ink_limit_px, p.cell_height_px, p.baseline_px.to_bits()],
                    dpr: self.dpr.to_bits(),
                };
                // Provisional source keys have no final store entry, even though
                // shared border/digit shapes make their screen cell nonblank.
                let ready = p.key.face != TOFU_FACE
                    && (context.backend.get(p.key).is_some()
                        || context.backend.source_color(p.key).is_some()
                        || context.raster.get(key).is_some());
                if let Some(line) = row.id {
                    cells.push(InkSample {
                        line,
                        row: index as u32,
                        column: p.cell_column,
                        span: u32::from(p.cell_span),
                        key: p.key,
                        ready,
                    });
                }
            }
        }
        cells
    }
}

/// Unpacks the frame's straight RGBA color without treating alpha as RGB contrast.
fn rgb(color: u32) -> [u8; 3] {
    [color as u8, (color >> 8) as u8, (color >> 16) as u8]
}

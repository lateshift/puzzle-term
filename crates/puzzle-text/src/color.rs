//! Skrifa COLRv0/v1 traversal into consecutive Slug layers (design §8.7).
//!
//! Clip paths are frozen in root em coordinates when pushed. Brush transforms
//! affect color sampling only, never the already captured clip geometry.
//! Unsupported composites keep source-over order; the caller logs once/font.
use kurbo::BezPath;
use puzzle_slug::paint::Paint;
/// One ordered color fill before atlas preparation.
#[derive(Clone, Debug)]
pub struct ColorLayer {
    /// Innermost clip outline in root em coordinates.
    pub path: BezPath,
    /// Other enclosing outlines intersected by the paint sampler.
    pub clips: Vec<BezPath>,
    /// Resolved palette colors and inverse brush coordinate map.
    pub paint: Paint,
    /// True if any palette reference uses COLR's foreground sentinel.
    pub foreground: bool,
}
/// Successful traversal with its one-time composite diagnostic.
pub struct ColorLayers {
    /// Fills remain in traversal order, which is also draw order.
    pub layers: Vec<ColorLayer>,
    /// At least one compositing mode was approximated as source-over.
    pub unsupported_composite: bool,
}
/// Brush normalization and premultiplied palette resolution.
mod brush;
/// Traversal state, clip scopes and error capture for void callbacks.
mod painter;
impl crate::face::FontFace {
    /// Paints a COLR glyph without hinting, or returns None for ordinary text.
    pub fn color_layers(&self, glyph: u32, foreground: u32) -> Result<Option<ColorLayers>, String> {
        if !self.has_colr() {
            return Ok(None);
        }
        let font = self.font()?;
        borrowed_layers(&font, self.location(), glyph, foreground)
    }
}
use skrifa::MetadataProvider;
#[cfg(test)]
mod tests;

/// Traverses COLR from a borrowed platform font, without retaining discovery types.
/// Both shaping and painting use our aligned skrifa location and table version.
/// The returned graph owns all geometry and colors, so the font can be released.
pub(crate) fn borrowed_layers(
    font: &skrifa::FontRef<'_>,
    location: &skrifa::instance::Location,
    glyph: u32,
    foreground: u32,
) -> Result<Option<ColorLayers>, String> {
    let Some(color) = font.color_glyphs().get(skrifa::GlyphId::new(glyph)) else {
        return Ok(None);
    };
    // COLR geometry always uses the same variation coordinates as shaping.
    let mut painter = painter::Painter::new(font, location, foreground)?;
    color
        .paint(location, &mut painter)
        .map_err(|e| format!("COLR traversal: {e}"))?;
    painter.finish().map(Some)
}

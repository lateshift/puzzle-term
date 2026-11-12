//! M4 test scenes composed from neutral grid placements and Slug output.
//!
//! The caller supplies platform-owned font bytes. Harfrust shapes each
//! line, `puzzle-text` positions it on integer device cells, and only then
//! does the D-28 backend produce Slug handles. The GPU sees an ordinary
//! batch of instances; no font data is embedded in this neutral crate.
//!
//! The constructor represents a narrow M4 specimen path, not the M6
//! session frame builder. It keeps the same ownership direction: platform
//! bytes enter `FontSet`, neutral layout emits placements, and the Slug
//! backend supplies handles. A future atlas backend can consume those
//! same placements without changing line origins or grid advances.
//!
//! This is also where §8.3 and §8.4 ink fitting happens after backend
//! preparation. A fixed font's synthetic bold draw, a wide fallback's
//! uniform shrink, and a narrow wide-cell glyph's centering affect only
//! its Slug instance transform. The neutral placement and next cell stay
//! exactly where shaping put them, preserving D-28's backend invariant.
use crate::glyph_ink::append_ink;
/// Shared GPU instance type re-exported across the shell boundary (§4.1).
pub use puzzle_frame::slug::SlugInstance;
use puzzle_slug::SlugAtlas;
// The platform shell receives only these composition-layer types; its
// manifest still depends on `puzzle-term` and `puzzle-render` (§4.1).
/// Counts Unicode cells with the same rule used by neutral text layout.
pub use puzzle_text::layout::cell_count;
pub use puzzle_text::{
    FontData,
    font_set::{FontSet, FontStyle},
    metrics::{CellMetrics, CellOptions, MetricAdjustment},
    pack::FontPack,
};
use puzzle_text::{
    glyph::{GlyphBackend, GlyphBackendSelector, PreparedGlyph, RendererChoice, slug::SlugBackend},
    hinting::HintingSetting,
    layout::{self, LayoutOptions},
    shape::TextShaper,
};

/// One text line requested by a headless M4 scenario.
pub struct TextLine<'a> {
    /// Source text, including spaces that advance empty cells.
    pub text: &'a str,
    /// Top-left cell position in device pixels.
    pub origin_px: [i32; 2],
    /// Logical font size before DPR.
    pub size: f32,
    /// Device pixels per logical pixel.
    pub dpr: f32,
    /// Primary face style.
    pub style: FontStyle,
    /// Whether coding ligatures may span neighbouring cells.
    pub ligatures: bool,
    /// Whether a rotation, shear or zoom requires original outlines.
    pub transformed: bool,
}

/// CPU atlas and GPU-ready instances for a deterministic text screenshot.
pub struct TextScene {
    /// Uploaded through the same Slug texture path as M3 scenes.
    pub atlas: SlugAtlas,
    /// One vector draw record per visible glyph and synthetic-bold layer.
    pub instances: Vec<SlugInstance>,
    /// Offscreen target width in device pixels.
    pub width: u32,
    /// Offscreen target height in device pixels.
    pub height: u32,
}

/// Builds text through the D-28 seam without a terminal grid model.
impl TextScene {
    /// Shapes and prepares all requested lines for one bounded GPU target.
    pub fn new(
        fonts: &mut FontSet,
        lines: &[TextLine<'_>],
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        // The GPU target bound is checked before outline preparation so
        // a malformed scenario cannot allocate an unpresentable atlas.
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err("text scene exceeds the 8192-pixel target limit".into());
        }
        let mut shaper = TextShaper::default();
        // Reusing one shaper across lines lets repeated style specimens
        // benefit from cached Harfrust plans and Unicode buffers.
        let mut backend = SlugBackend::default();
        let selector = GlyphBackendSelector {
            renderer: RendererChoice::Auto,
        };
        let mut instances = Vec::new();
        for line in lines {
            // One line may have several face runs, yet all of its cells
            // retain the primary face's integer metrics.
            // A synthetic italic fallback is an affine effect, so §8.5
            // requires its original, unhinted outline even at DPR one.
            let transformed = line.transformed || fonts.needs_synthetic_italic(line.style);
            let layout = layout::line(
                fonts,
                &mut shaper,
                line.text,
                LayoutOptions {
                    size: line.size,
                    dpr: line.dpr,
                    origin_px: line.origin_px,
                    style: line.style,
                    hinting: HintingSetting::Auto,
                    ligatures: line.ligatures,
                    features: &[],
                    cursor_byte: None,
                    transformed,
                    symbol_overflow: true,
                    builtin_glyphs: true,
                    color: 0xffff_ffff,
                },
            )?;
            // A placement records the origin and advance before any
            // vector preparation or renderer policy is applied.
            for placement in &layout.glyphs {
                let Some(handle) = prepare_handle(&selector, &mut backend, fonts, placement)?
                else {
                    continue;
                };
                // Fixed-weight fonts need an offset second ink layer in
                // bold style (§8.3); it never changes layout's advance.
                let synthetic_bold = fonts.needs_synthetic_bold(line.style, placement.key.face);
                // The primary monospace face may intentionally draw a
                // coding ligature across cells despite per-glyph clusters;
                // only fallback faces need §8.4 metric fitting here.
                let fallback = placement.key.face != fonts.primary_face(line.style);
                append_ink(
                    &mut instances,
                    placement,
                    handle,
                    transformed,
                    synthetic_bold,
                    fallback,
                );
            }
        }
        // Moving the atlas preserves handles already copied into the
        // instances; no second shape preparation is needed by the shell.
        Ok(Self {
            atlas: backend.into_atlas(),
            instances,
            width,
            height,
        })
    }
}

/// Resolves the M4 vector backend and skips empty shaping components.
fn prepare_handle(
    selector: &GlyphBackendSelector,
    backend: &mut SlugBackend,
    fonts: &mut FontSet,
    placement: &puzzle_text::glyph::GlyphPlacement,
) -> Result<Option<puzzle_slug::atlas::ShapeHandle>, String> {
    // Backend selection cannot change the already rounded cell origin.
    if selector.choose(placement) != puzzle_text::glyph::BackendKind::Vector {
        return Err("M4 text scene selected an unavailable atlas backend".into());
    }
    match backend.prepare(placement, fonts)? {
        PreparedGlyph::Vector(handle) => Ok(Some(handle)),
        PreparedGlyph::Empty | PreparedGlyph::Pending => Ok(None),
        PreparedGlyph::GridOutline { .. } => {
            Err("isolated vector scene selected a grid mask".into())
        }
        PreparedGlyph::Bitmap(_, _) => Err("M4 Slug backend returned a bitmap".into()),
        PreparedGlyph::Rasterized(_) => {
            Err("isolated M4 specimen exhausted vector capacity".into())
        }
    }
}

#[cfg(test)]
mod tests;

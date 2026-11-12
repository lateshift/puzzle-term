//! Backend-neutral glyph positions and selection (design §8.9, D-28).
//!
//! Layout decides where a glyph belongs in the terminal grid. Rendering
//! decides how its ink is produced. Keeping those decisions separate means
//! changing a hinter or rasterizer cannot move the cursor, selection or
//! decorations relative to the text.
//!
//! Phase 1 selects Slug for ordinary text, including D-27's hinted small
//! outlines. The bitmap result and rasterizer trait reserve the same seam
//! for browser and native fallback glyphs in later milestones. Effects
//! always select vectors because a fixed-size bitmap would blur on zoom.
use kurbo::BezPath;
use puzzle_slug::atlas::ShapeHandle;

/// A glyph's identity, with zero denoting a size-independent outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    /// Index of the face in the active font set.
    pub face: u32,
    /// OpenType glyph identifier within the face.
    pub glyph_id: u32,
    /// Variation-instance identifier within the face.
    pub instance: u32,
    /// Device ppem for a hinted outline, or zero for an unhinted one.
    pub hint_ppem: u16,
    /// Outline strategy; equal ppem with different hinters needs a new shape.
    pub hint_mode: HintMode,
}

/// Resolved outline strategy stored in a prepared glyph's cache key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HintMode {
    /// Original font curves remain valid for every device size.
    None,
    /// Skrifa's light auto-hinter fits stems at one device ppem.
    Light,
    /// Skrifa executes the font's own hinting instructions.
    Native,
}

/// One grid position that remains stable across rendering backends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphPlacement {
    /// The prepared shape or bitmap lookup identity.
    pub key: GlyphKey,
    /// Source column, independent of combining-mark offsets and backend ink.
    pub cell_column: u32,
    /// Whole-device-pixel origin of the cluster's first cell.
    pub origin_px: [i32; 2],
    /// Baseline measured from the row top, independent of absolute origin.
    /// Sized free text and effect rasters carry their own baseline to platform sources.
    pub baseline_px: f32,
    /// Device-pixel font size, before any backend-specific ink fitting.
    pub size_px: f32,
    /// Grid advance, independent of the font's shaped advance.
    pub advance_px: u32,
    /// Device-pixel width available for ink, including a blank overflow cell.
    pub ink_limit_px: u32,
    /// Device-pixel height available to fallback ink in its grid row.
    pub cell_height_px: u32,
    /// Number of cells owned by the source cluster.
    pub cell_span: u8,
    /// Straight RGBA8 text color, applied after glyph preparation.
    pub color: u32,
    /// Rotation, shear, scaling or animation requires a vector glyph.
    pub effect: bool,
    /// Color glyphs may use an RGBA bitmap instead of a coverage mask.
    pub color_glyph: bool,
}

/// Backend families supported by the D-28 selection seam.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    /// A size-independent Slug outline and handle.
    Vector,
    /// A size-specific coverage or color bitmap.
    Bitmap,
}

/// Explicit renderer selection from the future font settings schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RendererChoice {
    /// D-27 defaults to Slug for text at every device size.
    Auto,
    /// Force a Slug outline even when an atlas backend is present.
    Slug,
    /// Request an atlas for ordinary glyphs, retaining vectors for effects.
    Atlas,
}

/// Chooses a drawable family without modifying the input placement.
pub struct GlyphBackendSelector {
    /// Font setting that controls ordinary glyphs.
    pub renderer: RendererChoice,
}

/// Makes the D-27 default and the effect exception explicit in one place.
impl GlyphBackendSelector {
    /// Returns the backend for one glyph without changing layout fields.
    pub fn choose(&self, placement: &GlyphPlacement) -> BackendKind {
        if placement.effect {
            // A bitmap prepared at one ppem would blur or alias after an
            // affine effect, so effects retain the vector representation.
            return BackendKind::Vector;
        }
        match self.renderer {
            // The chosen light autohinter feeds Slug directly, avoiding an
            // atlas in the Phase 1 default (D-27).
            RendererChoice::Auto | RendererChoice::Slug => BackendKind::Vector,
            RendererChoice::Atlas => BackendKind::Bitmap,
        }
    }
}

/// Bitmap data produced by a future oracle or platform rasterizer.
pub struct RasterGlyph {
    /// Cropped bitmap origin relative to the placement baseline, in device pixels.
    pub offset_px: [i32; 2],
    /// Number of device pixels in one row.
    pub width: u32,
    /// Number of device-pixel rows.
    pub height: u32,
    /// Coverage or RGBA samples in row-major order.
    pub pixels: Vec<u8>,
    /// The atlas pipeline's interpretation of each pixel.
    pub kind: BitmapKind,
}

/// Channel interpretation reserved for the raster pipeline (§10.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitmapKind {
    /// One 8-bit alpha coverage value per pixel.
    Mask,
    /// Four RGBA8 components per pixel for color glyphs.
    Color,
    /// Three LCD coverage components; deferred until dual-source blending.
    Subpixel,
}

/// Location of a prepared bitmap in a future GPU atlas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtlasRect {
    /// First atlas column in device pixels.
    pub x: u32,
    /// First atlas row in device pixels.
    pub y: u32,
    /// Atlas columns occupied by this glyph.
    pub width: u32,
    /// Atlas rows occupied by this glyph.
    pub height: u32,
}

/// The backend-specific result attached to a neutral placement.
pub enum PreparedGlyph {
    /// An accepted cold job owns preparation; this frame draws a provisional box.
    Pending,
    /// A substituted component has no drawable contour but still owns cells.
    Empty,
    /// Packed Slug shape, consumed by an instanced vector draw.
    Vector(ShapeHandle),
    /// Atlas region and sampling mode, consumed by a raster draw.
    Bitmap(AtlasRect, BitmapKind),
    /// Smaller grid ink is rasterized in a bounded batch before uploads leave the text owner.
    GridOutline {
        /// Already converted geometry transfers once to the selected bitmap owner.
        ink: Box<crate::preparation::Outline>,
        /// Exact device placement determines fitting and crop geometry.
        placement: GlyphPlacement,
        /// Fitting, synthetic slant and duplicate-bold policy agree with vector emission.
        style: [bool; 3],
    },
    /// Capacity overflow preserves covered ink through the CPU oracle (D-39).
    /// The frame's raster atlas owns these samples once it accepts the insertion.
    Rasterized(RasterGlyph),
}

/// Supplies font outlines without exposing rendering policy to layout.
pub trait GlyphOutlineSource {
    /// Returns a chosen glyph's path in em space, or a validation report.
    fn outline(&mut self, key: GlyphKey) -> Result<BezPath, String>;
    /// Optional host admission transfers complete geometry rather than discarded validation.
    fn prepared_outline(
        &mut self,
        p: &GlyphPlacement,
        _style: [bool; 3],
    ) -> Option<Result<Option<crate::preparation::Outline>, String>> {
        Some(
            self.outline(p.key)
                .and_then(crate::preparation::Outline::new),
        )
    }
}

/// Turns a neutral placement into a drawable representation.
pub trait GlyphBackend {
    /// Prepares a glyph while preserving every placement coordinate.
    fn prepare(
        &mut self,
        placement: &GlyphPlacement,
        source: &mut dyn GlyphOutlineSource,
    ) -> Result<PreparedGlyph, String>;
}

/// Rasterizes through an optional oracle or platform plug-in.
pub trait GlyphRasterizer {
    /// Produces bounded bitmap samples for one untrusted font glyph.
    fn rasterize(
        &mut self,
        placement: &GlyphPlacement,
        source: &mut dyn GlyphOutlineSource,
    ) -> Result<RasterGlyph, String>;
}

/// Device-size CPU Slug masks used when one frame exhausts vector capacity.
pub mod oracle;
/// Slug's concrete Phase 1 vector backend.
pub mod slug;
#[cfg(test)]
mod tests;

//! CPU Slug rasterizer for covered glyphs that cannot fit the vector atlas (D-39).
//!
//! A reusable isolated atlas prepares only the current overflow outline. Sampling
//! uses the existing oracle math and the same fitting, shear and bold policy as
//! a vector instance. Crop offsets are relative to the neutral placement baseline.
//!
//! D-39 uses this after vector capacity rejection; D-47 also uses it for cold
//! grid masks in hosts without a GPU producer. The destination atlas retains the resulting mask so
//! later unchanged rows copy their raster quads without rerunning the oracle.
//! Its independent mirror holds only the outline currently being sampled.
//! That ownership separates CPU bitmap preparation from live GPU address lifetime.
//! A reset reuses scratch rows and advances their generation before each request.
//! Hinting has already happened in the outline source when the key requests it.
//! Consequently vector and bitmap backends receive the same contour geometry.
//! Fitting adjusts ink inside its existing span; layout origins and advances
//! remain immutable. Synthetic slant and weight mirror the vector emitter.
//! Each output byte stores coverage rather than terminal RGB, so selection and
//! cursor redraws can tint the same mask through the ordinary raster shader.
//! Crop size is validated before allocation and includes analytic edge coverage.
//! The live frame cannot be compacted or patched by this isolated rasterizer.
use super::{BitmapKind, GlyphOutlineSource, GlyphPlacement, GlyphRasterizer, RasterGlyph};
use puzzle_slug::{ShapeKey, SlugAtlas, atlas::FillRule, reference::sample};

/// Small scratch atlas never competes with the live frame's shape addresses.
#[derive(Default)]
pub struct OracleRasterizer {
    /// Reused mirrors contain one overflow outline at a time.
    atlas: SlugAtlas,
}

/// The optional bitmap backend remains behind the existing rasterizer seam (§8.9).
impl GlyphRasterizer for OracleRasterizer {
    /// Extracts the selected hinted/unhinted outline without modifying placement.
    fn rasterize(
        &mut self,
        p: &GlyphPlacement,
        source: &mut dyn GlyphOutlineSource,
    ) -> Result<RasterGlyph, String> {
        // The source owns font tables; the oracle consumes only its extracted contour.
        let path = source.outline(p.key)?;
        self.path(p, &path, false, false, false)
    }
}

/// Fitting and synthesis are ink policy, shared by native and bundled fallback.
impl OracleRasterizer {
    /// Produces a bounded mask at the requested device size using unchanged Slug coverage.
    pub fn path(
        &mut self,
        p: &GlyphPlacement,
        path: &kurbo::BezPath,
        fitting: bool,
        italic: bool,
        bold: bool,
    ) -> Result<RasterGlyph, String> {
        // Legacy callers provide a path; grid preparation transfers converted geometry instead.
        let ink = crate::preparation::Outline::new(path.clone())?.ok_or("empty oracle outline")?;
        self.prepared(p, ink, [fitting, italic, bold])
    }

    /// Samples already converted geometry without repeating extraction or band choice.
    pub fn prepared(
        &mut self,
        p: &GlyphPlacement,
        ink: crate::preparation::Outline,
        style: [bool; 3],
    ) -> Result<RasterGlyph, String> {
        // Overflow retains its original crop policy and reviewed reference images.
        self.sample_prepared(p, ink, style, false)
    }
    /// Samples the same oracle with the native grid's tighter rounded coverage crop.
    pub fn grid(
        &mut self,
        p: &GlyphPlacement,
        ink: crate::preparation::Outline,
        style: [bool; 3],
    ) -> Result<RasterGlyph, String> {
        // Grid shelves omit the legacy extra empty fringe while retaining a transparent gutter.
        self.sample_prepared(p, ink, style, true)
    }
    /// Keeps coverage evaluation shared while crop policy leaves legacy overflow unchanged.
    fn sample_prepared(
        &mut self,
        p: &GlyphPlacement,
        ink: crate::preparation::Outline,
        style: [bool; 3],
        tight: bool,
    ) -> Result<RasterGlyph, String> {
        let [fitting, italic, bold] = style;
        // The scratch mirror contains only this glyph; its allocated rows are reused.
        self.atlas.reset().map_err(|e| e.to_string())?;
        let handle = self
            .atlas
            .prepare_quadratics(
                ShapeKey(1),
                ink.shape,
                ink.bands,
                FillRule::NonZero,
                self.atlas.generation(),
            )
            .map_err(|e| e.to_string())?;
        // Only the crop changes: both policies use identical inverse transforms and coverage.
        let grid = if tight {
            geometry::SampleGrid::grid(p, handle.bbox, style)?
        } else {
            geometry::SampleGrid::new(p, handle.bbox, fitting, italic, bold)?
        };
        // A cold glyph owns its tightly packed result until the bounded raster cache takes it.
        let mut pixels = Vec::with_capacity((grid.width * grid.height) as usize);
        for y in 0..grid.height {
            for x in 0..grid.width {
                // Invert the exact em→device fit at each sample center, as the GPU does.
                let at = grid.at(x, y);
                let mut coverage =
                    sample(&self.atlas, handle, at, grid.derivative).map_err(|e| e.to_string())?;
                if bold {
                    // Source-over matches the second synthetic-bold vector draw.
                    let second = sample(
                        &self.atlas,
                        handle,
                        [at[0] - grid.bold / grid.size, at[1]],
                        grid.derivative,
                    )
                    .map_err(|e| e.to_string())?;
                    coverage += second * (1.0 - coverage);
                }
                pixels.push((coverage * 255.0).round() as u8);
            }
        }
        Ok(RasterGlyph {
            offset_px: grid.offset,
            width: grid.width,
            height: grid.height,
            pixels,
            kind: BitmapKind::Mask,
        })
    }
}

/// Ordered color fills and enclosing clips use the same CPU coverage oracle.
mod color;
/// Device crop and inverse affine sampling are shared by mask and color overflow.
pub(crate) mod geometry;

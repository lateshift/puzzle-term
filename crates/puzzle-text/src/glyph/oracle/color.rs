//! COLR overflow evaluates ordered source-over layers with the existing paint oracle.
//!
//! Each paint graph has one root em coordinate system. Preparing every clip
//! before sampling keeps a child's brush transform registered with its parents.
//! The same union bounds used by vector layers determine a single device fit.
//! Layer-local fitting would move colored components relative to their outlines.
//! This scratch atlas is independent of the live frame's captured addresses.
//! Resetting it cannot invalidate a shape already queued for GPU submission.
//! Its generation provides an advancing preparation clock for each graph.
//! The graph is temporary: only its final straight RGBA crop is retained.
//! RasterAtlas copies that crop into its bounded color pages and transparent gutter.
//! Palette colors retain their authored RGB; only OpenType foreground sentinels
//! inherit the placement's resolved terminal foreground before CPU evaluation.
//! Source-over uses premultiplied channels, matching both Slug and raster shaders.
//! Transparent samples therefore carry zero RGB and cannot create colored fringes.
//! Every enclosing clip multiplies alpha rather than replacing an inner contour.
//! Gradient extension and interpolation come from Paint::sample, the existing
//! oracle used to compare GPU color-glyph output in M11.
//! No vector layer is published until the complete graph has fitted the live atlas.
//! If that fails, this path renders the entire graph instead of mixing backends
//! for individual layers, which would change ordering against neighboring glyphs.
use super::{BitmapKind, GlyphPlacement, OracleRasterizer, RasterGlyph, geometry::SampleGrid};
use crate::color::ColorLayer;
use puzzle_slug::{ShapeKey, atlas::FillRule, paint::Paint, reference::sample};

/// Prepared ordered fills and their common fitting bounds.
type PreparedGraph = (Vec<(puzzle_slug::ShapeHandle, Paint)>, [f32; 4]);

/// Whole graphs are rasterized together so layer fitting cannot move their registration.
impl OracleRasterizer {
    /// Composites premultiplied layers and returns straight device-sized RGBA samples.
    pub fn color(
        &mut self,
        p: &GlyphPlacement,
        layers: &[ColorLayer],
    ) -> Result<RasterGlyph, String> {
        // Retained scratch capacity is reused, but no graph may borrow prior handles.
        self.atlas.reset().map_err(|e| e.to_string())?;
        // All shape/paint dependencies precede pixel evaluation and share one fit.
        let (prepared, bounds) = self.prepare_layers(p, layers)?;
        let mut fitted = *p;
        // Color ink receives the common emoji inset from the existing fitting policy.
        fitted.color_glyph = true;
        let grid = SampleGrid::new(&fitted, bounds, true, false, false)?;
        let mut pixels = Vec::with_capacity((grid.width * grid.height * 4) as usize);
        // Device sample centers invert the same matrix used by vector instances.
        for y in 0..grid.height {
            for x in 0..grid.width {
                let at = grid.at(x, y);
                // A fresh transparent destination preserves the graph's layer order.
                let mut output = [0.0; 4];
                for (shape, paint) in &prepared {
                    // Every enclosing clip multiplies the same root-space Slug coverage.
                    let mut coverage = sample(&self.atlas, *shape, at, grid.derivative)
                        .map_err(|e| e.to_string())?;
                    for clip in &paint.clips {
                        coverage *= sample(&self.atlas, *clip, at, grid.derivative)
                            .map_err(|e| e.to_string())?;
                    }
                    let ink = paint.sample(at).map(|v| v * coverage);
                    // The destination is already premultiplied; never multiply RGB twice.
                    for i in 0..4 {
                        output[i] = ink[i] + output[i] * (1.0 - ink[3]);
                    }
                }
                // Composite in premultiplied space, then return straight RGBA because
                // the raster shader premultiplies each texel before interpolation.
                // Quantization happens only after the whole ordered graph is complete.
                if output[3] > 0.0 {
                    for i in 0..3 {
                        output[i] /= output[3];
                    }
                }
                pixels.extend(output.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
            }
        }
        Ok(RasterGlyph {
            // Crop offsets remain relative to the original neutral baseline.
            offset_px: grid.offset,
            width: grid.width,
            height: grid.height,
            pixels,
            kind: BitmapKind::Color,
        })
    }
    /// Packs a small isolated graph before any device sampling starts.
    fn prepare_layers(
        &mut self,
        p: &GlyphPlacement,
        layers: &[ColorLayer],
    ) -> Result<PreparedGraph, String> {
        // Cold graph storage is bounded by the incoming font's validated layer list.
        let mut prepared = Vec::with_capacity(layers.len());
        // Positive/negative infinities seed a union rather than adding an artificial origin.
        let mut bounds = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        let mut identity = 0;
        // Fills and outer clips use one allocator so their handles cannot alias.
        for layer in layers {
            identity += 1;
            let shape = self
                .atlas
                .prepare(
                    ShapeKey(identity),
                    &layer.path,
                    FillRule::NonZero,
                    self.atlas.generation(),
                )
                .map_err(|e| e.to_string())?;
            // Union fill bounds before fitting; outer clips can only reduce coverage.
            for i in 0..2 {
                bounds[i] = bounds[i].min(shape.bbox[i]);
                bounds[i + 2] = bounds[i + 2].max(shape.bbox[i + 2]);
            }
            let mut paint = layer.paint.clone();
            // Keep brush metadata owned until this whole crop has been sampled.
            for clip in &layer.clips {
                identity += 1;
                paint.clips.push(
                    self.atlas
                        .prepare(
                            ShapeKey(identity),
                            clip,
                            FillRule::NonZero,
                            self.atlas.generation(),
                        )
                        .map_err(|e| e.to_string())?,
                );
            }
            resolve_foreground(&mut paint, p.color);
            // Keep font layer order; sorting by brush would change source-over pixels.
            prepared.push((shape, paint));
        }
        Ok((prepared, bounds))
    }
}

/// White reference sentinels inherit the placement foreground before CPU paint evaluation.
fn resolve_foreground(paint: &mut Paint, color: u32) {
    // The placement carries straight RGBA; paint evaluation expects premultiplied color.
    let foreground = color.to_le_bytes().map(|v| f32::from(v) / 255.0);
    if paint.foreground
        && let puzzle_slug::paint::Brush::Solid(rgba) = &mut paint.brush
    {
        // Foreground alpha combines terminal opacity with the font's paint opacity.
        rgba[3] *= foreground[3];
        for i in 0..3 {
            rgba[i] = foreground[i] * rgba[3];
        }
    }
    for stop in &mut paint.stops {
        // Fixed palette stops remain unchanged under terminal foreground selection.
        if stop.foreground {
            stop.color[3] *= foreground[3];
            for (i, channel) in foreground.iter().enumerate().take(3) {
                stop.color[i] = channel * stop.color[3];
            }
        }
    }
}

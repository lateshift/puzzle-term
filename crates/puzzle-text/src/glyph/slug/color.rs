//! COLR geometry/paint preparation sharing SlugBackend's existing atlas.
//!
//! Complete glyph layers are cached, not regenerated per terminal cell. The
//! whole glyph's union bounds determine fitting, so its layers stay aligned.
//!
//! A color glyph cannot use the ordinary one-key/one-outline cache: each
//! fill may reference a different glyph outline, brush transform and clip
//! scope. The prepared value therefore owns an ordered slice of layer draws.
//! Each draw still uses exactly one 56-byte Slug instance in the final frame.
//! The innermost clip supplies the quad; additional clips live in its paint
//! row and evaluate the same band/curve coverage at the same root coordinate.
//! Solids put straight sRGB in the instance and leave the row to clip alpha.
//! Foreground solids retain their paint opacity separately from the text RGB.
//! A gradient remains in the row because its color changes across the quad.
//!
//! Shape and paint handles both carry generations. A hit must validate both
//! before a row can use the cached addresses. Rebuilding a shape alone would
//! be insufficient because a paint record contains outer clip addresses too.
//! Invalid input never publishes a partially prepared ordered layer slice.
//! Atlas IDs are assigned monotonically alongside normal glyph shape IDs,
//! so a COLR clip cannot accidentally reuse an ordinary glyph's atlas key.
use super::SlugBackend;
use crate::{
    font_set::FontSet,
    glyph::{GlyphKey, GlyphPlacement},
};
use puzzle_slug::{ShapeHandle, ShapeKey, atlas::FillRule, paint::table::PaintHandle};
use std::collections::HashMap;
/// Shader-facing color layer with stale paint and shape address guards.
#[derive(Clone, Copy, Debug)]
pub struct ColorDraw {
    /// Innermost clip path, transformed into the glyph's root em space.
    pub shape: ShapeHandle,
    /// Nonzero shader paint index, including clip and brush metadata.
    pub paint: PaintHandle,
    /// Solid palette color carried by the instance; foreground brushes use None.
    pub solid_color: Option<u32>,
    /// Foreground solids multiply only the text alpha by this paint opacity.
    pub opacity: f32,
}
/// Cached complete glyph layers plus their union bounds.
#[derive(Clone, Debug)]
pub struct PreparedColor {
    /// Ordered source-over fills; never reordered by palette or shape.
    pub layers: Vec<ColorDraw>,
    /// One fit applies to every layer, preserving COLR registration.
    pub bounds: [f32; 4],
}
/// Bounded glyph cache and CPU paint mirror owned by the shared text context.
#[derive(Default)]
pub(super) struct ColorCache {
    /// Geometry is shared across foreground colors; the shader resolves tint.
    pub(super) glyphs: HashMap<(GlyphKey, u32), PreparedColor>,
    /// Mirror rows serve upload, device restoration and color-oracle tests.
    pub(super) paints: puzzle_slug::paint::table::PaintTable,
    /// Monotonic build clock, independent of terminal output or wall time.
    pub(super) frame: u64,
    /// Once-per-shared-font diagnostics, including duplicate primary styles.
    warned: std::collections::HashSet<(usize, u32)>,
}
impl SlugBackend {
    /// Performs paint eviction before the host captures changed VT rows.
    /// Relocation cannot occur after a row has copied a shader paint address.
    /// The host uses the returned flag to invalidate all row-cache epochs.
    pub fn begin_color_frame(&mut self) -> Result<bool, String> {
        // One shared build clock ages text from every Session using this context.
        self.colors.frame = self.colors.frame.saturating_add(1);
        // Page eviction occurs only here, before the host captures row-cache epochs.
        let shapes = self
            .atlas
            .begin_frame(self.colors.frame)
            .map_err(|e| e.to_string())?;
        if shapes {
            // Glyph maps never retain keys whose geometry has already been evicted.
            // Registered pack IDs survive page pressure while startup pins remain.
            self.shapes.retain(|_, key| self.atlas.get(*key).is_some());
            self.vector_sizes
                .retain(|key, _| self.shapes.contains_key(key));
            self.source_styles
                .retain(|key, _| self.shapes.contains_key(key));
        }
        Ok(self.colors.paints.begin_frame(self.colors.frame)? || shapes)
    }
    /// Keeps drawn cached rows live even when their VT content did not change.
    /// The zero paint index belongs to ordinary text and needs no table touch.
    /// Assembled instances account for cache hits as well as freshly shaped rows.
    pub fn mark_color_used(&mut self, index: u32) {
        if index != 0 {
            self.colors.paints.mark_index(index, self.colors.frame);
        }
    }

    /// Gets or prepares a complete COLRv0/v1 glyph in consecutive layer order.
    pub fn prepare_color(
        &mut self,
        placement: &GlyphPlacement,
        fonts: &FontSet,
    ) -> Result<Option<&PreparedColor>, String> {
        // A parsed monochrome face cannot have a color-cache hit or a COLR graph.
        // Check its immutable capability before hashing or crossing the painter seam.
        // Source-only worker faces have no FontFace and still use the loaded cache.
        if fonts
            .face(placement.key.face)
            .is_some_and(|face| !face.has_colr())
        {
            return Ok(None);
        }
        // White is an opacity reference; the foreground sentinel uses live tint.
        let key = (placement.key, 0xffffffff);
        // Reuse the owned slice by reference: a warm row performs no layer Vec clone.
        if self.colors.glyphs.get(&key).is_some_and(|cached| {
            cached.layers.iter().all(|l| {
                self.atlas.contains_handle(l.shape)
                    && (l.paint.index == 0 || self.colors.paints.contains(l.paint))
            })
        }) {
            return Ok(self.colors.glyphs.get(&key));
        }
        if fonts.preparation_scheduled() {
            // A missing live graph is prepared once through the same owned worker job
            // as an outline, never synchronously by this preliminary color probe.
            return Ok(None);
        }
        // Procedural/source glyph keys are handled through their own backend path.
        let Some(face) = fonts.face(placement.key.face) else {
            return Ok(None);
        };
        // Monochrome faces return immediately through their precomputed COLR flag.
        let Some(color) = face.color_layers(placement.key.glyph_id, 0xffffffff)? else {
            return Ok(None);
        };
        // Regular/bold instances may share bytes; pointer/index identity coalesces logs.
        if color.unsupported_composite && self.colors.warned.insert(face.identity()) {
            tracing::warn!(
                event = "font.colr_composite_approximated",
                "non-source-over COLR composite rendered as source-over"
            );
        }
        // Retained glyph descriptors are bounded independently of texture row memory.
        // Stale records contain obsolete addresses; pruning them bounds descriptors.
        let paints = &self.colors.paints;
        self.colors.glyphs.retain(|_, glyph| {
            glyph
                .layers
                .iter()
                .all(|l| l.paint.index == 0 || paints.contains(l.paint))
        });
        if self.colors.glyphs.len() >= 4096 {
            return Err("COLR glyph cache reached 4096 entries".into());
        }
        let prepared = self.color_layers(color.layers)?;
        // Publish only after every clip and paint has a valid prepared address.
        self.colors.glyphs.insert(key, prepared);
        Ok(self.colors.glyphs.get(&key))
    }
    /// Publishes a worker-traversed graph without probing the source face again.
    pub fn prepare_loaded_color(
        &mut self,
        p: &GlyphPlacement,
        color: crate::color::ColorLayers,
    ) -> Result<Option<crate::glyph::RasterGlyph>, String> {
        // A worker graph has no borrowed FontFace; the stable source face ID
        // coalesces its approximation warning across sizes and terminal rows.
        if color.unsupported_composite && self.colors.warned.insert((p.key.face as usize, 0)) {
            tracing::warn!(
                event = "font.colr_composite_approximated",
                "non-source-over COLR composite rendered as source-over"
            );
        }
        // Discard descriptors whose pages were cleared before bounding new graph entries.
        let atlas = &self.atlas;
        let paints = &self.colors.paints;
        self.colors.glyphs.retain(|_, g| {
            g.layers.iter().all(|l| {
                atlas.contains_handle(l.shape) && (l.paint.index == 0 || paints.contains(l.paint))
            })
        });
        if self.colors.glyphs.len() >= 4096
            && !self.colors.glyphs.contains_key(&(p.key, 0xffffffff))
        {
            // This limit bounds heap descriptors, independently of the 48-MiB
            // texel budget. Replacing an existing key consumes no new descriptor.
            return Err("COLR glyph cache reached 4096 entries".into());
        }
        let original = color.layers;
        // The original owned graph is retained only through this packing attempt,
        // so capacity overflow does not discard validation and repeat traversal.
        match self.color_layers(original.clone()) {
            Ok(prepared) => {
                // Only a complete graph becomes visible; a partially packed graph
                // must never expose clip addresses from an unsuccessful attempt.
                self.colors.glyphs.insert((p.key, 0xffffffff), prepared);
                Ok(None)
            }
            // The frame's protected pages stay put. The same validated graph supplies
            // device-sized oracle ink until a later boundary can reclaim a page.
            Err(_) if self.atlas.needs_eviction() => self.rasterize_color(p, &original).map(Some),
            Err(error) => Err(error),
        }
    }
    /// Packs a complete glyph graph, retaining one union box for layer fitting.
    pub(super) fn color_layers(
        &mut self,
        layers: Vec<crate::color::ColorLayer>,
    ) -> Result<PreparedColor, String> {
        // All layers use one union box; independent per-layer fitting distorts emoji.
        let mut prepared = PreparedColor {
            layers: Vec::new(),
            bounds: [
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ],
        };
        for mut layer in layers {
            let shape = self.color_shape(&layer.path)?;
            // Clip scopes share the same root coordinates, including transformed boxes.
            for clip in &layer.clips {
                layer.paint.clips.push(self.color_shape(clip)?);
            }
            // Root em bounds are measured before the common device-pixel fit.
            for i in 0..2 {
                prepared.bounds[i] = prepared.bounds[i].min(shape.bbox[i]);
                prepared.bounds[i + 2] = prepared.bounds[i + 2].max(shape.bbox[i + 2]);
            }
            // Convert premultiplied CPU palette color back to the instance convention.
            let (solid_color, opacity) = match layer.paint.brush {
                puzzle_slug::paint::Brush::Solid(rgba) => {
                    let fixed = (!layer.paint.foreground).then(|| pack_color(rgba));
                    let alpha = if layer.paint.foreground { rgba[3] } else { 1.0 };
                    // Solids carry their color in the instance. The paint row
                    // contributes only enclosing clip coverage to those fills.
                    layer.paint.brush = puzzle_slug::paint::Brush::Solid([1.0; 4]);
                    layer.paint.foreground = true;
                    (fixed, alpha)
                }
                _ => (None, 1.0),
            };
            // Unclipped solids need no paint fetch; their instance carries all RGBA.
            let paint = if matches!(layer.paint.brush, puzzle_slug::paint::Brush::Solid(_))
                && layer.paint.clips.is_empty()
            {
                PaintHandle {
                    generation: 0,
                    index: 0,
                }
            } else {
                self.color_paint(&layer.paint)?
            };
            prepared.layers.push(ColorDraw {
                shape,
                paint,
                solid_color,
                opacity,
            });
        }
        Ok(prepared)
    }

    /// Allocates brush/clip metadata only when the instance cannot carry it alone.
    /// Solids without outer clips bypass this method and use paint index zero.
    /// A clipped solid still needs a row for frozen enclosing clip handles.
    /// Gradients always need the row for their geometry and color-stop data.
    /// The live insert preserves every previously captured address this frame.
    pub(super) fn color_paint(
        &mut self,
        paint: &puzzle_slug::paint::Paint,
    ) -> Result<PaintHandle, String> {
        // Geometry and paint IDs share one monotonic collision-free allocator.
        self.next_shape = self
            .next_shape
            .checked_add(1)
            .ok_or("COLR paint identity exhausted")?;
        self.colors
            .paints
            .prepare_live(self.next_shape, paint, self.colors.frame)
    }

    /// Drains paint rows only into a frame that retains them until GPU writes.
    pub fn take_paint_uploads(&mut self) -> Vec<(u32, Vec<[f32; 4]>)> {
        // The frame owns these dirty rows until queue writes acknowledge them (§10.4).
        self.colors.paints.take_dirty()
    }
    /// Marks the retained paint mirror for a newly created device texture.
    pub fn restore_paints(&mut self) {
        self.colors.paints.restore();
    }
    /// Packs one root-em clip shape, retaining the existing atlas safety checks.
    fn color_shape(&mut self, path: &kurbo::BezPath) -> Result<ShapeHandle, String> {
        self.next_shape = self
            .next_shape
            .checked_add(1)
            .ok_or("COLR shape identity exhausted")?;
        self.atlas
            .prepare(
                ShapeKey(self.next_shape),
                path,
                FillRule::NonZero,
                self.colors.frame,
            )
            .map_err(|e| e.to_string())
    }
}

/// Encodes a premultiplied palette color as straight sRGB for the instance.
fn pack_color(rgba: [f32; 4]) -> u32 {
    // Transparent palette colors have no meaningful straight RGB component.
    let a = rgba[3];
    if a <= 0.0 {
        return 0;
    }
    let bytes = [
        (rgba[0] / a * 255.0).round() as u8,
        (rgba[1] / a * 255.0).round() as u8,
        (rgba[2] / a * 255.0).round() as u8,
        (a * 255.0).round() as u8,
    ];
    u32::from_le_bytes(bytes)
}

#[cfg(test)]
/// Palette and warm-cache contracts through the live backend surface.
mod tests;

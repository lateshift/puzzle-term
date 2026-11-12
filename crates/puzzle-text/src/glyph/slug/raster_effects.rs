//! Bitmap-only affine matrices and tint rows in the shared paint mirror (§8.9).
//!
//! The raster instance retains its forty-byte format. Its tint word becomes a
//! paint index under the effect flag, and the vertex stage reads the forward
//! matrix from texel one and premultiplied color from texel five. These are
//! existing paint-row fields, interpreted only by the raster effect branch.
//! Ordinary Slug brushes keep their inverse-transform interpretation unchanged.
//!
//! Metadata depends on the linear map and tint, not the glyph or translation.
//! A transformed run therefore shares one row across all its bitmap clusters.
//! Generations invalidate handles before frame capture, just as for COLR rows.
//! Cold pruning bounds descriptors without scanning them on a warmed hit.
//!
//! Vector variants resolve foreground references before changing opacity, so
//! authored palette colors keep their own alpha. Explicit recolors replace RGB
//! while multiplying the original alpha, rather than flattening color layers.
//! Source addresses and exact opacity bits identify this resolved paint variant.
//! New keys come from the backend's common counter to avoid font/overlay aliases.
//! The row allocator refuses overflow instead of relocating captured draw indices.
//! Raster matrices have no translation: each quad carries its own final origin.
//! Tint alpha applies to authored-color samples; tint RGB applies only to masks
//! or an explicit recolor, preserving the native emoji palette by default.
use super::SlugBackend;
use puzzle_slug::paint::{Brush, Extend, Paint, table::PaintHandle};
use std::collections::HashMap;

/// Exact shader coefficient bits and straight RGBA identify reusable metadata.
type Key = ([u32; 4], u32);
/// Descriptor count and texture memory have independent bounds.
#[derive(Default)]
pub(super) struct Cache {
    /// Generational rows for complete affine/tint requests.
    rows: HashMap<Key, PaintHandle>,
    /// Vector paint variants depend on the current source address and foreground tint.
    effects: HashMap<(u32, u32, Option<u32>, u32), PaintHandle>,
}
/// Matrix rows use the same allocator and retention clock as vector paints.
impl SlugBackend {
    /// Reuses a bitmap effect's linear map and alpha for an isolated cursor mask.
    /// Only RGB changes; authored emoji samples still ignore tint RGB in WGSL.
    pub fn raster_cursor(&mut self, index: u32, color: u32) -> Result<u32, String> {
        let (matrix, original) = self
            .raster_effects
            .rows
            .iter()
            .find(|(_, handle)| handle.index == index && self.colors.paints.contains(**handle))
            .map(|(key, _)| *key)
            .ok_or("cursor bitmap effect expired")?;
        // Effect alpha survives cursor contrast recoloring just as it does for vectors.
        self.prepare_raster_effect(
            matrix.map(f32::from_bits),
            (color & 0x00ffffff) | (original & 0xff000000),
        )
    }

    /// Applies ink-only opacity/recoloring to an existing COLR gradient or clipped solid.
    pub fn prepare_paint_effect(
        &mut self,
        source: u32,
        foreground: u32,
        color: Option<u32>,
        opacity: f32,
    ) -> Result<u32, String> {
        let key = (source, foreground, color, opacity.to_bits());
        if let Some(h) = self.raster_effects.effects.get(&key).copied()
            && self.colors.paints.contains(h)
        {
            self.mark_color_used(h.index);
            return Ok(h.index);
        }
        // A compacted mirror expires every variant before an index can name another paint.
        let paints = &self.colors.paints;
        self.raster_effects
            .effects
            .retain(|_, h| paints.contains(*h));
        if self.raster_effects.effects.len() >= 4096 {
            return Err("effect paint descriptor allowance exceeded".into());
        }
        self.next_shape = self
            .next_shape
            .checked_add(1)
            .ok_or("paint key exhausted")?;
        let id = self.next_shape;
        let handle = self.colors.paints.prepare_effect(
            id,
            source,
            foreground,
            color,
            opacity,
            self.colors.frame,
        )?;
        self.raster_effects.effects.insert(key, handle);
        Ok(handle.index)
    }
    /// Prepares forward matrix/tint metadata, returning the shader paint index.
    pub fn prepare_raster_effect(&mut self, matrix: [f32; 4], color: u32) -> Result<u32, String> {
        // Reject NaNs before their bits could key a cache or reach a vertex position.
        if !matrix.iter().all(|v| v.is_finite()) {
            return Err("bitmap effect matrix is nonfinite".into());
        }
        let key = (matrix.map(f32::to_bits), color);
        if let Some(handle) = self.raster_effects.rows.get(&key).copied()
            && self.colors.paints.contains(handle)
        {
            // Retention follows actual draws, including unchanged provider lists.
            self.mark_color_used(handle.index);
            return Ok(handle.index);
        }
        // Expired rows no longer reserve descriptors; live rows remain shared.
        let paints = &self.colors.paints;
        self.raster_effects.rows.retain(|_, h| paints.contains(*h));
        if self.raster_effects.rows.len() >= 4096 {
            return Err("bitmap effect descriptor allowance exceeded".into());
        }
        let rgba = color.to_le_bytes().map(|v| f32::from(v) / 255.0);
        // The shader filters coverage and multiplies this premultiplied tint afterward.
        let paint = Paint {
            brush: Brush::Solid([
                rgba[0] * rgba[3],
                rgba[1] * rgba[3],
                rgba[2] * rgba[3],
                rgba[3],
            ]),
            foreground: false,
            // Only raster vertices interpret this field as a forward linear matrix.
            inverse: [matrix[0], matrix[1], matrix[2], matrix[3], 0.0, 0.0],
            extend: Extend::Pad,
            stops: Vec::new(),
            clips: Vec::new(),
        };
        // Live insertion cannot relocate addresses already captured by this frame.
        let handle = self.color_paint(&paint)?;
        self.raster_effects.rows.insert(key, handle);
        Ok(handle.index)
    }
}

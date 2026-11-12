//! Per-surface image textures: lazy uploads, byte-bounded LRU and recovery (M15.3).
//!
//! Images never occupy glyph atlas pages. The cache moves with a surface when
//! shared font contexts are lent to siblings or changed after DPR/font updates.
//! Weak identities do not keep sources live; dead entries retire at the next frame.
//! Replacing the device drops this rebuildable cache; sealed source pixels then
//! upload again on demand. No readback or parser-side GPU work is involved.
use crate::{RenderError, Renderer};
use puzzle_frame::{ImageSource, PipelineKind, RasterInstance, RenderFrame};
use std::{ops::Range, sync::Weak};
/// Cropped image quads are expanded only at hardware tile boundaries.
mod geometry;
/// Device creation and source tiling are cold paths, outside warm assembly.
mod upload;

/// One hardware tile, including neighbor texels for seamless bilinear filtering.
struct Tile {
    /// Source origin and dimensions, in original image pixels.
    rect: [u32; 4],
    /// Uploaded rectangle includes one neighboring texel on each internal edge.
    upload: [u32; 4],
    /// Bound source texture survives every in-flight submission.
    binding: wgpu::BindGroup,
}
/// Residency belongs to one source generation, not a reusable wire ID.
struct Entry {
    /// Weak allocation identity prevents stale ID hits without owning a live source.
    pixels: Weak<[u8]>,
    /// Dimensions also participate in identity for safe public raster sources.
    size: [u32; 2],
    /// Sum of tile RGBA allocations, including their filtering borders.
    bytes: usize,
    /// Logical frame ordinal used for recency eviction, never wall-clock tuning.
    used: u64,
    /// Images larger than the device limit split into independent exact textures.
    tiles: Vec<Tile>,
}
/// One expanded tile draw references its original frame instance.
struct Draw {
    /// Original raster-array offset identifies the painter batch it belongs to.
    original: u32,
    /// Expanded quad index in the image-only instance buffer.
    instance: u32,
    /// A handle clone retains sampled pixels until this frame is submitted.
    binding: wgpu::BindGroup,
}
/// Rebuildable, independently budgeted GPU state for one terminal surface.
/// Hosts move this value with their surface; it is not a font-context resource.
pub struct ImageCache {
    /// Device identity rejects resources retained across a device-loss replacement.
    device: wgpu::Device,
    /// Stable projection belongs to this surface, independent of shared text policy.
    viewport: wgpu::Buffer,
    /// Same explicit layout as the existing raster pipeline, not a fourth shader.
    layout: wgpu::BindGroupLayout,
    /// Unused coverage binding satisfies the common raster ABI with one texel.
    mask: wgpu::Texture,
    /// Images have no glyph effects, but the common ABI still has a paint binding.
    paint: wgpu::TextureView,
    /// Nearest sampler is unused by image filtering but required by the layout.
    sampler: wgpu::Sampler,
    /// Only sources already seen in a visible placement have residency entries.
    entries: Vec<Entry>,
    /// Exact image bytes, independently limited from decoded and glyph storage.
    bytes: usize,
    /// Frame recency increments only when this surface prepares a frame.
    clock: u64,
    /// Retained expanded geometry avoids allocations in warmed frames (P1).
    quads: Vec<RasterInstance>,
    /// Painter routing retains its allocation after an image scrolls offscreen.
    draws: Vec<Draw>,
    /// Persistent image-only instance buffer uses the existing forty-byte ABI.
    instances: wgpu::Buffer,
    /// Geometric buffer growth is independent of source residency accounting.
    capacity: u32,
}
/// Residency and expanded geometry live on the render owner, never on Session.
impl ImageCache {
    /// Uploads visible sources, evicting hidden sources first by recency.
    pub(crate) fn prepare(
        &mut self,
        renderer: &Renderer,
        frame: &RenderFrame,
        target: [u32; 2],
    ) -> Result<(), RenderError> {
        // A presentation ordinal defines recency without a timer or periodic sweeping.
        self.clock = self.clock.wrapping_add(1);
        // Retain geometry capacity even after the last placement leaves the viewport.
        self.quads.clear();
        self.draws.clear();
        // Only frame-builder-admitted visible sources are candidates for residency.
        let sources = frame
            .images
            .as_ref()
            .map_or(&[][..], |images| images.sources.as_slice());
        let budget = frame.images.as_ref().map_or(0, |images| images.budget);
        // Residency admission finishes before any draw can reference a missing texture.
        self.residency(renderer, sources, budget)?;
        // Expand only image batches. Glyph instance indices and atlas flags stay untouched.
        for batch in &frame.batches {
            if let PipelineKind::Image(index) = batch.pipeline {
                let source = sources
                    .get(index as usize)
                    .ok_or_else(|| RenderError::Upload("missing image source".into()))?;
                let entry = self
                    .entries
                    .iter()
                    .find(|e| e.matches(source))
                    .ok_or_else(|| RenderError::Upload("missing resident image".into()))?;
                for original in batch.instances.clone() {
                    let quad = frame
                        .rasters
                        .get(original as usize)
                        .ok_or_else(|| RenderError::Upload("image instance out of range".into()))?;
                    geometry::expand(entry, *quad, original, &mut self.quads, &mut self.draws);
                }
            }
        }
        // Geometry and projection are ready together for the acquired drawable size.
        self.write_instances(renderer, target);
        Ok(())
    }
    /// Pins the current sources before evicting hidden generations by recency.
    fn residency(
        &mut self,
        renderer: &Renderer,
        sources: &[ImageSource],
        budget: usize,
    ) -> Result<(), RenderError> {
        let limit = renderer.device().limits().max_texture_dimension_2d;
        // Checked admission includes hardware seam borders, never just compressed input.
        let required = sources.iter().try_fold(0usize, |bytes, source| {
            bytes.checked_add(puzzle_frame::image_resident_bytes(source.size(), limit))
        });
        if required.is_none_or(|bytes| bytes > budget) {
            return Err(RenderError::Upload(
                "visible image sources exceed configured residency allowance".into(),
            ));
        }
        // Mark every visible generation before considering a live budget decrease.
        for entry in &mut self.entries {
            if sources.iter().any(|source| entry.matches(source)) {
                entry.used = self.clock;
            }
        }
        // New bytes must fit alongside all current hits before any upload begins.
        // The required sum above also proves this subset sum cannot overflow.
        let missing: usize = sources
            .iter()
            .filter(|source| !self.entries.iter().any(|entry| entry.matches(source)))
            .map(|source| puzzle_frame::image_resident_bytes(source.size(), limit))
            .sum();
        // Reserve room by retiring hidden entries first; pinned hits are not rebuilt.
        self.evict(budget - missing);
        for source in sources {
            if !self.entries.iter().any(|entry| entry.matches(source)) {
                // Upload once per immutable source, regardless of placement count.
                let entry = self.upload(renderer, source)?;
                self.bytes += entry.bytes;
                self.entries.push(entry);
            }
        }
        Ok(())
    }
    /// Retires hidden least-recently-used images; current visible entries stay pinned.
    fn evict(&mut self, budget: usize) {
        // A dead weak identity cannot hit again, even without memory pressure.
        self.entries.retain(|entry| {
            let keep = entry.pixels.strong_count() != 0;
            if !keep {
                self.bytes -= entry.bytes;
            }
            keep
        });
        // An image that is merely hidden can stay cached until pressure requires space.
        // A live shrink uses the same recency policy as admitting a new source.
        while self.bytes > budget {
            let Some(index) = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.used != self.clock)
                .min_by_key(|(_, entry)| entry.used)
                .map(|(index, _)| index)
            else {
                break;
            };
            self.bytes -= self.entries.swap_remove(index).bytes;
        }
    }
    /// Draws exact source tiles in their original painter range through the raster shader.
    pub(crate) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        pipeline: &wgpu::RenderPipeline,
        range: Range<u32>,
    ) -> u32 {
        // Images reuse the raster shader while binding their own texture and vertices.
        pass.set_pipeline(pipeline);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        let mut count = 0;
        for draw in self
            .draws
            .iter()
            .filter(|draw| range.contains(&draw.original))
        {
            // Queue order preserves each texture until its submitted draw is finished.
            pass.set_bind_group(0, &draw.binding, &[]);
            pass.draw(0..6, draw.instance..draw.instance + 1);
            count += 1;
        }
        count
    }
    /// Moving between font contexts preserves images, but device loss cannot.
    pub(crate) fn is_device(&self, renderer: &Renderer) -> bool {
        self.device == *renderer.device()
    }
    /// Reports actual source texture bytes, excluding small common shader resources.
    pub fn resident_bytes(&self) -> usize {
        self.bytes
    }
}
/// Generation identity combines immutable dimensions with the retained allocation.
impl Entry {
    /// Weak pointers retain allocation identity even after the final source Arc is dropped.
    fn matches(&self, source: &ImageSource) -> bool {
        // Weak keeps this allocation reserved, preventing address reuse (ABA).
        // evict() drops dead identities on the next frame, releasing that reservation.
        self.size == source.size()
            && std::ptr::addr_eq(self.pixels.as_ptr(), source.pixels().as_ptr())
    }
}

#[cfg(test)]
/// Readback witnesses cover actual independent image sampling.
mod tests;

//! Immutable image sources and raster commands, independent of GPU or VT ownership.
//!
//! Pixels are shared with sealed snapshots, never copied per placement. Identity
//! comparisons use the allocation, not its contents: different transmissions of
//! the same wire ID must never reuse stale GPU ink (M15.3, D-50).
//!
//! This contract stores source identity separately from each destination quad.
//! Several placements can therefore share one upload without comparing their
//! potentially large pixel arrays. Source rectangles are normalized edges; the
//! destination and scissor are device pixels after the frame builder applies DPR.
//! The contract contains no texture handle, font state or mutable terminal data.
//! Its Arc retains exactly the source generation used by the sealed publication.
use crate::{Batch, PipelineKind, RasterInstance, RectU32, RenderFrame, ZSlot};
use std::sync::Arc;

/// A validated straight-alpha RGBA8 source retained until its last publication retires.
#[derive(Clone, Debug)]
pub struct ImageSource {
    /// Physical source dimensions, independent of terminal DPR and font metrics.
    size: [u32; 2],
    /// Immutable row-major pixels, also the generation-safe cache identity.
    pixels: Arc<[u8]>,
}
/// All GPU consumers receive dimensions and bytes that were validated together.
impl ImageSource {
    /// Rejects empty, overflowing or incomplete images before GPU upload.
    pub fn new(size: [u32; 2], pixels: Arc<[u8]>) -> Option<Self> {
        // checked_mul also rejects products that cannot fit the target address space.
        let bytes = (size[0] as usize)
            .checked_mul(size[1] as usize)?
            .checked_mul(4)?;
        (size[0] != 0 && size[1] != 0 && bytes == pixels.len()).then_some(Self { size, pixels })
    }
    /// Width and height of the original decoded image, in source pixels.
    pub fn size(&self) -> [u32; 2] {
        self.size
    }
    /// Shared immutable bytes let a GPU cache retain only a weak identity.
    pub fn pixels(&self) -> &Arc<[u8]> {
        &self.pixels
    }
}
/// Cache equality deliberately means identity rather than bytewise image equality.
impl PartialEq for ImageSource {
    /// Allocation identity avoids hashing or comparing every pixel on warm frames.
    fn eq(&self, other: &Self) -> bool {
        self.size == other.size && Arc::ptr_eq(&self.pixels, &other.pixels)
    }
}
/// An image and a normalized source crop used by the reserved overlay primitive.
#[derive(Clone, Debug, PartialEq)]
pub struct RasterRef {
    /// Pixels survive parser-side deletion until this sealed frame is retired.
    pub image: ImageSource,
    /// Normalized left/top/right/bottom within the original decoded source.
    pub uv: [f32; 4],
}
/// Lazy per-frame image state; image-free frames allocate nothing here (P1).
#[derive(Default)]
pub struct FrameImages {
    /// Same allowance as decoded storage, independently accounted on the GPU.
    pub budget: usize,
    /// Only actually visible image sources are eligible for lazy GPU upload.
    pub sources: Vec<ImageSource>,
}
/// Raster commands use the same batching/scissor contract as ordinary grid ink.
impl RenderFrame {
    /// Appends one raster primitive in device pixels, preserving painter order.
    pub fn append_image(
        &mut self,
        source: &RasterRef,
        rect: [f32; 4],
        slot: ZSlot,
        clip: Option<RectU32>,
    ) {
        // Reject nonfinite geometry and invalid crops at the public extension seam.
        if !rect.iter().chain(&source.uv).all(|n| n.is_finite())
            || rect[2] <= 0.0
            || rect[3] <= 0.0
            || source.uv[0] < 0.0
            || source.uv[1] < 0.0
            || source.uv[2] > 1.0
            || source.uv[3] > 1.0
            || source.uv[2] <= source.uv[0]
            || source.uv[3] <= source.uv[1]
        {
            return;
        }
        // Upload only sources whose destination intersects the current content clip.
        let bounds = clip.unwrap_or(RectU32 {
            x: 0,
            y: 0,
            width: self.target_size[0],
            height: self.target_size[1],
        });
        if rect[0] >= bounds.x.saturating_add(bounds.width) as f32
            || rect[1] >= bounds.y.saturating_add(bounds.height) as f32
            || rect[0] + rect[2] <= bounds.x as f32
            || rect[1] + rect[3] <= bounds.y as f32
        {
            return;
        }
        // Allocate the extension only after visibility is established: empty images
        // and offscreen overlays do not create a source table or a GPU upload.
        let images = self.images.get_or_insert_with(Default::default);
        // Equal identities share one table entry even when crops or z values differ.
        let index = images
            .sources
            .iter()
            .position(|image| image == &source.image)
            .unwrap_or_else(|| {
                // One immutable handle clone is enough for the lifetime of this frame.
                images.sources.push(source.image.clone());
                images.sources.len() - 1
            });
        let start = self.rasters.len() as u32;
        // Bit three selects premultiplied filtering without the glyph-effect matrix.
        self.rasters.push(RasterInstance {
            rect,
            uv: source.uv,
            tint: u32::MAX,
            flags: 9,
        });
        // Preserve the source index on the batch, leaving RasterInstance at forty bytes.
        let pipeline = PipelineKind::Image(index as u32);
        // Adjacent tiles with the same source and clip are one instanced draw.
        // Their individual UVs stay in the instance array; merging only extends
        // the draw range, keeping one texture binding for all tiles of that image.
        // Never merge across another primitive: painter order must remain exact.
        if let Some(last) = self.batches.last_mut()
            && last.slot == slot
            && last.pipeline == pipeline
            && last.scissor == clip
            && last.instances.end == start
        {
            last.instances.end = start + 1;
        } else {
            self.batches.push(Batch {
                slot,
                pipeline,
                instances: start..start + 1,
                scissor: clip,
            });
        }
    }
    /// Stable allocation-free slot ordering also applies when no overlay is installed.
    pub fn order_batches(&mut self) {
        // Insertion sorting is stable: equal-z paper/image primitives keep their
        // authored painter order, and the usually short batch list needs no scratch Vec.
        for index in 1..self.batches.len() {
            let mut current = index;
            while current > 0 && self.batches[current - 1].slot > self.batches[current].slot {
                self.batches.swap(current - 1, current);
                current -= 1;
            }
        }
    }
}

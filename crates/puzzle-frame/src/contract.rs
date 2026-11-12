//! Owned frame contract between the neutral session and wgpu (§10.2).
//!
//! The session builds only plain instances and texture spans. It never sees
//! a device or surface (R9). Batches name ordered z-slots so the renderer
//! can issue a few instanced draws without sorting individual glyphs.
//! Reusing these vectors after each frame is the P1 allocation guarantee.
use std::ops::Range;

use crate::{RasterInstance, RectInstance, SlugInstance};

/// Global transform values shared by every instance in one frame.
#[derive(Clone, Copy, Debug)]
pub struct Globals {
    /// Four clip-space matrix rows for device-pixel object coordinates.
    pub projection: [[f32; 4]; 4],
    /// Current device-pixel ratio for transformed overlays.
    pub dpr: f32,
    /// Animation time supplied by the session's manual or platform clock.
    pub time_seconds: f32,
}

impl Default for Globals {
    /// Starts with a neutral transform; the builder sets the viewport rows.
    fn default() -> Self {
        Self {
            projection: [[0.0; 4]; 4],
            dpr: 1.0,
            time_seconds: 0.0,
        }
    }
}

/// Inclusive minimum and exclusive maximum clipping box in device pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RectU32 {
    /// Left edge of the clipped area.
    pub x: u32,
    /// Top edge of the clipped area.
    pub y: u32,
    /// Width within the render target.
    pub width: u32,
    /// Height within the render target.
    pub height: u32,
}

/// The three fixed render pipelines of §10.3.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipelineKind {
    /// Filled and stroked rectangular regions.
    Rect,
    /// Slug's analytic outline coverage.
    Slug,
    /// R8 coverage or RGBA atlas pixels.
    Raster,
    /// Same raster shader, sampling an independent visible image source.
    Image(u32),
}

/// Stable painter's order for terminal and overlay content (§10.3).
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ZSlot {
    /// Cell fills, selection and search backgrounds.
    Background = 0,
    /// Overlay content below text ink.
    BelowText = 1,
    /// Vector glyphs and line decorations.
    Text = 2,
    /// Bitmap glyphs and color emoji.
    TextRaster = 3,
    /// Cursor shape and optional inverted glyph.
    Cursor = 4,
    /// Annotations and vector badges above text.
    AboveText = 5,
    /// Unclipped interface elements above the grid.
    Top = 6,
}

/// A contiguous instance range drawn with one pipeline and scissor.
#[derive(Clone, Debug)]
pub struct Batch {
    /// Painter's order; batches with equal slots keep submission order.
    pub slot: ZSlot,
    /// Instance array selected by this batch.
    pub pipeline: PipelineKind,
    /// Half-open range within that array.
    pub instances: Range<u32>,
    /// Optional grid clip, omitted for top-level interface elements.
    pub scissor: Option<RectU32>,
}

/// Dirty CPU mirror span to transfer before its draw batch.
///
/// Coordinates use the destination atlas's texels, not device pixels.
#[derive(Clone, Debug)]
pub enum Upload {
    /// Starts native frame-local raster staging; retained redraws keep it until the next marker.
    RasterReset,
    /// Complete page contents that must precede their stamped draw segment.
    RasterPage {
        /// Selects the RGBA array rather than the R8 coverage array.
        color: bool,
        /// Physical atlas layer, reused only after earlier draws finish.
        page: u32,
        /// Bits 16–31 of raster instances select this immutable page image.
        epoch: u16,
        /// Full 2048-square page staged for ordered buffer-to-texture copies.
        pixels: Vec<u8>,
    },
    /// Copies already rasterized GPU coverage without reading it onto the CPU.
    GpuMask {
        /// Device-local ticket held by the host renderer until upload acknowledgement.
        ticket: u64,
        /// Shared neutral lifetime marker retains a staging source through retries and skips.
        lease: std::sync::Arc<()>,
        /// Top-left crop in that GPU batch's R8 coverage sheet.
        source: [u32; 2],
        /// Existing sampled mask layer.
        page: u32,
        /// Destination excludes the shelf's transparent border.
        rect: RectU32,
    },
    /// Captures an existing GPU-only page before later queue writes reuse its layer.
    RasterCapture {
        /// Authored color and coverage arrays have independent indices.
        color: bool,
        /// Physical layer whose current contents must survive the following clear.
        page: u32,
        /// Draw instances select the captured content with this stamp.
        epoch: u16,
    },
    /// Clears a recycled GPU-only page, including abandoned ink and gutters.
    RasterClear {
        /// Physical mask layer; GPU grid coverage never targets the color array.
        page: u32,
    },
    /// RGBA32Float Slug curve rows, already packed by puzzle-slug.
    Curves {
        /// First texel in the curve texture.
        origin: [u32; 2],
        /// Four float components per texel.
        texels: Vec<[f32; 4]>,
    },
    /// RG16Uint Slug band-index rows.
    Bands {
        /// First texel in the band texture.
        origin: [u32; 2],
        /// Two unsigned components per texel.
        texels: Vec<[u16; 2]>,
    },
    /// R8 coverage bytes from a bitmap glyph producer.
    Mask {
        /// Array layer within the mask atlas.
        page: u32,
        /// Atlas rectangle receiving the upload.
        rect: RectU32,
        /// One byte per pixel in row-major order.
        pixels: Vec<u8>,
    },
    /// RGBA8 color bytes from an emoji or image producer.
    Color {
        /// Array layer within the color atlas.
        page: u32,
        /// Atlas rectangle receiving the upload.
        rect: RectU32,
        /// Four bytes per pixel in row-major order.
        pixels: Vec<u8>,
    },
    /// Float paint table row for a future gradient producer.
    Paint {
        /// First table entry receiving these colors.
        first: u32,
        /// Linear RGBA paint records.
        colors: Vec<[f32; 4]>,
    },
}

/// One complete, GPU-independent frame with reusable backing storage.
#[derive(Default)]
pub struct RenderFrame {
    /// Width and height of the target in device pixels.
    pub target_size: [u32; 2],
    /// Premultiplied clear color in render-target components.
    pub clear: [f32; 4],
    /// Projection, DPR and animation clock for this frame.
    pub globals: Globals,
    /// Pending atlas/paint writes retained across skipped frames until GPU acknowledgement.
    pub uploads: Vec<Upload>,
    /// Background, decoration and cursor geometry.
    pub rects: Vec<RectInstance>,
    /// Vector glyph and shape geometry.
    pub slugs: Vec<SlugInstance>,
    /// Bitmap glyph and image geometry.
    pub rasters: Vec<RasterInstance>,
    /// Ordered draw ranges; usually six to ten per frame.
    pub batches: Vec<Batch>,
    /// Lazy image payload avoids any allocation in image-free frames.
    pub images: Option<Box<crate::FrameImages>>,
}

impl RenderFrame {
    /// Replaces image geometry, preserving unwritten uploads and hot-path allocations.
    pub fn clear_instances(&mut self) {
        // Instance arrays are reused even when a row cache changes which
        // runs are visible, so a full redraw needs no new frame vectors.
        // Instances describe just this image, but uploads also serve cached glyph
        // handles in later images. Only a successful GPU writer may clear them.
        self.rects.clear();
        self.slugs.clear();
        self.rasters.clear();
        self.batches.clear();
        if let Some(images) = &mut self.images {
            // Retain source-vector capacity, but release obsolete pixel lifetimes.
            images.sources.clear();
        }
    }
}

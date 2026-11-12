//! Device-side mirrors of the Slug curve and band texture planes (§9.8).
//!
//! The CPU atlas remains authoritative across GPU device loss. Its dirty
//! spans name row-local texel ranges, which can be sent with one queue write
//! per span. Rows double when a glyph exceeds current capacity; a GPU copy
//! preserves existing data before the new spans are written to the new view.
//!
//! Curves use Rgba32Float by D-8 and bands use Rg16Uint as in the reference.
//! Both are unfilterable texture bindings and remain 4096 texels wide. The
//! initial 64-row allocation agrees with the CPU atlas's default reserve.
//!
//! A band header stores an offset from the shape's glyph origin. Those
//! addresses remain unchanged when texture height increases, since width
//! never changes. The GPU copy therefore transfers the old rectangle to
//! the same upper-left coordinates of the new allocation. Only new CPU
//! spans require queue writes after that copy. If the device is replaced,
//! a fresh SlugTextures must receive a full mirror upload rather than an
//! incremental dirty list; the CPU atlas still has all source texels.
use crate::RenderError;
use puzzle_slug::{
    SlugAtlas,
    atlas::{DirtySpan, TEXTURE_WIDTH},
};
/// Native GPU readback verifies growth preserves old and new texels.
/// Frame-contract uploads stay separate from whole-atlas synchronization.
mod frame_upload;
#[cfg(feature = "oracle-debug")]
mod growth;
/// Test-only binary16 conversion and uploads for the D-8 cost comparison.
#[cfg(feature = "oracle-debug")]
mod half;

/// One typed GPU texture and its bindable view at a power-of-two row count.
///
/// The view is stored with its texture because a bind group cannot sample
/// an allocation after it has been replaced during growth.
struct Plane {
    /// GPU allocation holding one Slug texel format.
    texture: wgpu::Texture,
    /// Recreated with the allocation so bind groups never retain stale views.
    view: wgpu::TextureView,
    /// Current row capacity, starting at 64.
    height: u32,
    /// Fixed texel format for this plane.
    format: wgpu::TextureFormat,
    /// Used to size each row-local queue write.
    bytes_per_texel: u32,
    /// Labels identify a failed GPU operation in validation output.
    label: &'static str,
}

/// Curve and band data for one Slug atlas generation.
///
/// The planes are independent so a glyph with many curves but few band
/// entries does not double both allocations at the same time.
pub struct SlugTextures {
    /// Float curve control-point pairs.
    curves: Plane,
    /// Unsigned band headers and curve-location lists.
    bands: Plane,
}

/// Creates a sampleable and copyable 4096-wide Slug plane.
///
/// Integer band texels and floating curve texels share dimensions, but
/// their formats have different byte widths and different shader types.
fn texture(
    device: &wgpu::Device,
    label: &'static str,
    format: wgpu::TextureFormat,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    // One mip and one layer preserve the atlas's integer texel addresses;
    // filtering or mip selection would change quadratic control points.
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: TEXTURE_WIDTH as u32,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        // Growth copies from the old allocation, incremental writes target
        // the new one, and the shader samples the resulting complete plane.
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

/// Capacity and row-local transfer operations shared by the two formats.
impl Plane {
    /// Reserves the 64 rows available in an empty CPU atlas.
    fn new(
        device: &wgpu::Device,
        label: &'static str,
        format: wgpu::TextureFormat,
        bytes_per_texel: u32,
    ) -> Self {
        // The CPU atlas reserves 64 rows before any shape is packed, so GPU
        // coordinates can be uploaded without a capacity check on first use.
        let (texture, view) = texture(device, label, format, 64);
        Self {
            texture,
            view,
            height: 64,
            format,
            bytes_per_texel,
            label,
        }
    }

    /// Doubles rows and GPU-copies the existing plane before replacing its view.
    ///
    /// The copy is recorded as a GPU command instead of downloading old
    /// texels. This keeps growth off the CPU readback path during a frame.
    fn ensure_rows(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        required: u32,
    ) -> Result<(), RenderError> {
        // No rebind is needed when the existing texture still covers the
        // highest written CPU row; dirty spans alone handle that frame.
        if required <= self.height {
            return Ok(());
        }
        let mut next = self.height;
        // Power-of-two growth bounds allocation frequency as glyph count
        // grows, while packed addresses stay fixed across reallocations.
        while next < required {
            next = next
                .checked_mul(2)
                .ok_or_else(|| RenderError::Upload("Slug texture height overflow".into()))?;
        }
        if next > device.limits().max_texture_dimension_2d {
            return Err(RenderError::Upload(
                "Slug texture exceeds device dimension limit".into(),
            ));
        }
        // The new view must replace the old bind-group view only after the
        // GPU copy has been enqueued against both still-live textures.
        let (new_texture, new_view) = texture(device, self.label, self.format, next);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grow Slug texture"),
        });
        // Addresses are stable across growth, so a whole-plane copy avoids
        // retransmitting prior glyphs from the CPU mirror.
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &new_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: TEXTURE_WIDTH as u32,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));
        // wgpu retains resources referenced by the submitted command buffer
        // until it completes, so replacing these handles is safe now.
        self.texture = new_texture;
        self.view = new_view;
        self.height = next;
        Ok(())
    }

    /// Writes only spans changed by the CPU atlas since the previous frame.
    ///
    /// Each span is one row by construction. Queue::write_texture accepts
    /// its tightly packed bytes without the 256-byte stride required by a
    /// texture-to-buffer copy used for screenshot readback.
    fn write_spans<T: bytemuck::Pod>(
        &self,
        queue: &wgpu::Queue,
        texels: &[T],
        spans: &[DirtySpan],
    ) -> Result<(), RenderError> {
        for span in spans {
            // Dirty ranges are row-local, unlike a shape's logical band
            // block, which may cross rows through CalcBandLoc addressing.
            let start = usize::from(span.row) * TEXTURE_WIDTH + usize::from(span.start);
            let end = usize::from(span.row) * TEXTURE_WIDTH + usize::from(span.end);
            let slice = texels
                .get(start..end)
                .ok_or_else(|| RenderError::Upload("dirty Slug span exceeds CPU mirror".into()))?;
            let width = u32::from(span.end - span.start);
            // bytemuck views the existing mirror bytes without a temporary
            // allocation or a platform-specific float serialization path.
            // Queue writes accept a tightly packed one-row source. C1 and
            // B1 keep these positions in their own physical texture row.
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: u32::from(span.start),
                        y: u32::from(span.row),
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(slice),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * self.bytes_per_texel),
                    rows_per_image: Some(1),
                },
                wgpu::Extent3d {
                    width,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
        }
        Ok(())
    }
}

/// Owns both GPU stores and consumes the CPU atlas's dirty list.
impl SlugTextures {
    /// Uploads a validated font pack without a parsed TTF or CPU atlas.
    pub fn sync_pack(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        curves: &[[f32; 4]],
        bands: &[[u16; 2]],
    ) -> Result<(), RenderError> {
        // Pack rows are already laid out under C1/B1/B2 by xtask.
        // The same GPU formats receive their bytes as runtime glyphs.
        let curve_rows = curves.len().div_ceil(TEXTURE_WIDTH).max(64) as u32;
        let band_rows = bands.len().div_ceil(TEXTURE_WIDTH).max(64) as u32;
        self.curves.ensure_rows(device, queue, curve_rows)?;
        self.bands.ensure_rows(device, queue, band_rows)?;
        // A pack upload always writes all used rows; it cannot rely on a
        // runtime atlas's incremental dirty list or a previous device.
        self.curves
            .write_spans(queue, curves, &full_spans(curves.len()))?;
        self.bands
            .write_spans(queue, bands, &full_spans(bands.len()))
    }
    /// Allocates the reference's two planes without requiring optional features.
    pub fn new(device: &wgpu::Device) -> Self {
        // Float precision is D-8's measured default; unsigned bands must
        // remain exact because the shader interprets them as coordinates.
        Self {
            curves: Plane::new(
                device,
                "Slug curve texture",
                wgpu::TextureFormat::Rgba32Float,
                16,
            ),
            bands: Plane::new(
                device,
                "Slug band texture",
                wgpu::TextureFormat::Rg16Uint,
                4,
            ),
        }
    }

    /// Uses the reference's half-float curve format for a native D-8 probe.
    #[cfg(feature = "oracle-debug")]
    pub fn new_half(device: &wgpu::Device) -> Self {
        Self {
            curves: Plane::new(
                device,
                "Slug half curve texture",
                wgpu::TextureFormat::Rgba16Float,
                8,
            ),
            bands: Plane::new(
                device,
                "Slug band texture",
                wgpu::TextureFormat::Rg16Uint,
                4,
            ),
        }
    }

    /// Grows both planes if needed, then uploads the rows newly written by CPU preparation.
    ///
    /// Page eviction leaves surviving addresses unchanged and creates no upload.
    /// Reusing a cleared page marks only new texels; stale page generations
    /// invalidate row handles before any addresses are collected.
    pub fn sync(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &mut SlugAtlas,
    ) -> Result<(), RenderError> {
        // Used texels, rather than reserved CPU capacity, determine whether
        // the GPU must grow. Empty reserve rows need not be transferred.
        let curve_rows = atlas.curve_texels().len().div_ceil(TEXTURE_WIDTH).max(64) as u32;
        let band_rows = atlas.band_texels().len().div_ceil(TEXTURE_WIDTH).max(64) as u32;
        self.curves.ensure_rows(device, queue, curve_rows)?;
        self.bands.ensure_rows(device, queue, band_rows)?;
        // Each plane grows independently; their address spaces share width
        // but do not need to have equal heights at any instant.
        // Drain only after capacity checks succeed, so an allocation error
        // leaves dirty spans available for a later retry or device recovery.
        let (curve_dirty, band_dirty) = atlas.take_dirty();
        self.curves
            .write_spans(queue, atlas.curve_texels(), &curve_dirty)?;
        self.bands
            .write_spans(queue, atlas.band_texels(), &band_dirty)
    }

    /// Uploads complete CPU mirrors into newly allocated GPU textures.
    ///
    /// Incremental dirty lists belong to an existing GPU generation. A
    /// fresh offscreen scene or device replacement has no prior texels, so
    /// it cannot use only spans written since the last presentation.
    pub fn sync_full(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &mut SlugAtlas,
    ) -> Result<(), RenderError> {
        let curve_rows = atlas.curve_texels().len().div_ceil(TEXTURE_WIDTH).max(64) as u32;
        let band_rows = atlas.band_texels().len().div_ceil(TEXTURE_WIDTH).max(64) as u32;
        self.curves.ensure_rows(device, queue, curve_rows)?;
        self.bands.ensure_rows(device, queue, band_rows)?;
        // Physical rows, including any packed gaps, are copied so the GPU
        // address space exactly mirrors every retained CPU page after device loss.
        let curve_spans = full_spans(atlas.curve_texels().len());
        let band_spans = full_spans(atlas.band_texels().len());
        #[cfg(feature = "oracle-debug")]
        if self.curves.format == wgpu::TextureFormat::Rgba16Float {
            // The benchmark's source geometry is unchanged; only texel
            // precision and bandwidth differ from the production plane.
            self.curves
                .write_half_spans(queue, atlas.curve_texels(), &curve_spans)?;
        } else {
            self.curves
                .write_spans(queue, atlas.curve_texels(), &curve_spans)?;
        }
        #[cfg(not(feature = "oracle-debug"))]
        self.curves
            .write_spans(queue, atlas.curve_texels(), &curve_spans)?;
        self.bands
            .write_spans(queue, atlas.band_texels(), &band_spans)?;
        let _ = atlas.take_dirty();
        Ok(())
    }

    /// Exposes the two views in the same binding order as the WGSL source.
    pub fn views(&self) -> (&wgpu::TextureView, &wgpu::TextureView) {
        (&self.curves.view, &self.bands.view)
    }
}

/// Covers each used physical row with one queue-write range.
fn full_spans(texels: usize) -> Vec<DirtySpan> {
    (0..texels.div_ceil(TEXTURE_WIDTH))
        .map(|row| DirtySpan {
            row: row as u16,
            start: 0,
            end: (texels - row * TEXTURE_WIDTH).min(TEXTURE_WIDTH) as u16,
        })
        .collect()
}

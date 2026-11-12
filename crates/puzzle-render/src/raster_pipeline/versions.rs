//! Capacity-triggered staging for ordered draw, page reuse and continuation.
//!
//! A copy buffer owns one earlier page image. The sampled textures keep their
//! normal combined 48-MiB bound. Copies occur outside render passes, after
//! earlier draws have been submitted, so queue writes cannot erase their ink.
//! Retained readback and idle calibration may execute the same frame again;
//! buffers therefore live until the next native RasterReset upload, not until
//! the first presentation. Final copies restore the CPU mirror's incarnation.
//!
//! Mask rows contain coverage, color rows contain straight RGBA. The existing
//! shader still handles premultiplication and tint; staging never changes ink.
//! No sampled layer is added when a frame exceeds capacity. Version buffers
//! are temporary upload ownership, released on the next frame marker. They are
//! kept through skipped acquisition so retry and diagnostic readback see the
//! same complete image. A marker precedes that frame's versions; pending upload
//! streams may include earlier markers, with the final frame replacing them.
//! Full-page copies include all transparent gutters and cannot leak a prior
//! shelf's pixels into the next layout. Queue order replaces a CPU fence: the
//! previous segment is submitted before the copy that reuses its sampled layer.
//!
//! GPU-generated grid coverage uses the same protocol without a CPU readback.
//! A capture copies the current sampled layer into a device-local buffer before
//! its following clear. Both operations use one queue, so no CPU fence is needed.
//! Source captures also work on mixed pages containing authored CPU samples:
//! the CPU mirror alone cannot reconstruct the generated parts of that page.
//! The copy buffer has neither MAP_READ nor MAP_WRITE usage. It exists solely
//! to restore immutable coverage while an earlier draw segment is submitted.
//! The final incarnation is restored after the last segment, keeping warm cache
//! hits attached to the samples named by their unchanged shelf coordinates.
use super::{PAGE, RasterPipeline};
use crate::RenderError;
use puzzle_frame::Upload;

/// GPU copy source paired with a neutral instance's page-content stamp.
struct Version {
    /// Encodes format, layer and incarnation using the instance flag layout.
    stamp: u32,
    /// Full rows are already multiples of WebGPU's 256-byte copy alignment.
    buffer: wgpu::Buffer,
}

/// Frame-local copy sources; ordinary frames retain an empty vector.
#[derive(Default)]
pub(super) struct Versions {
    /// Upload order ends with the final image for each reused layer.
    images: Vec<Version>,
}

/// Accepts only explicit streaming uploads; ordinary crops use the parent writer.
impl Versions {
    /// Validates complete page bytes and transfers their ownership to copy buffers.
    pub(super) fn apply(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        upload: &Upload,
    ) -> Result<(), RenderError> {
        if matches!(upload, Upload::RasterReset) {
            // An empty follow-up render call has no marker and keeps retained sources.
            self.images.clear();
            return Ok(());
        }
        let Upload::RasterPage {
            color,
            page,
            epoch,
            pixels,
        } = upload
        else {
            return Ok(());
        };
        let channels = if *color { 4 } else { 1 };
        // The ABI reserves eight layer bits; sampled capacity is checked by ensure_page.
        if *page >= 256 || pixels.len() != (PAGE * PAGE * channels) as usize {
            return Err(RenderError::Upload(
                "invalid raster page incarnation".into(),
            ));
        }
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            // This buffer is a copy source only; the fragment shader never binds it.
            label: Some("ordered raster page copy source"),
            size: pixels.len() as u64,
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Queue owns these bytes before the first draw segment is submitted.
        queue.write_buffer(&buffer, 0, pixels);
        self.images.push(Version {
            // Preserve the instance ABI: low appearance bits remain free for effects.
            stamp: (u32::from(*epoch) << 16) | (*page << 8) | u32::from(*color),
            buffer,
        });
        Ok(())
    }
}

/// GPU page snapshots use the same immutable copy buffers as authored CPU pages.
impl Versions {
    /// Captures current GPU ink before any subsequent upload can reuse this layer.
    pub(super) fn capture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        page: u32,
        epoch: u16,
        color: bool,
    ) {
        // Format selection sets both the allocation size and copy row pitch.
        let channels = if color { 4 } else { 1 };
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("GPU-only ordered raster page snapshot"),
            size: u64::from(PAGE * PAGE * channels),
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false, // Device-local snapshot, never CPU pixel ownership.
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        // Full R8/RGBA rows already satisfy the copy buffer's 256-byte pitch.
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: page,
                },
                ..texture.as_image_copy()
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    // One complete layer starts at byte zero in its private snapshot.
                    offset: 0,
                    bytes_per_row: Some(PAGE * channels),
                    rows_per_image: Some(PAGE),
                },
            },
            wgpu::Extent3d {
                // Include empty shelves and gutters, not just the occupied glyph rectangles.
                width: PAGE,
                height: PAGE,
                depth_or_array_layers: 1,
            },
        );
        // No map or wait: the following clear is ordered after this submitted snapshot.
        queue.submit(Some(encoder.finish()));
        self.images.push(Version {
            // The same stamp interpretation applies to authored and GPU-only snapshots.
            stamp: (u32::from(epoch) << 16) | (page << 8) | u32::from(color),
            buffer,
        });
    }
}

/// Executes page-copy instructions without exposing textures to the text layer.
impl RasterPipeline {
    /// Whether this retained frame needs ordered streaming instead of one pass.
    pub(crate) fn has_versions(&self) -> bool {
        !self.versions.images.is_empty()
    }

    /// Restores one immutable page image before its following draw segment.
    pub(crate) fn copy_version(&self, encoder: &mut wgpu::CommandEncoder, stamp: u32) {
        // Effect/recolor flags do not select page contents.
        let stamp = stamp & 0xffff_ff01;
        // Unreused layers have no staged image and remain on their ordinary crop writes.
        if let Some(version) = self.versions.images.iter().find(|v| v.stamp == stamp) {
            self.copy_image(encoder, version);
        }
    }

    /// Leaves physical layers matching the current CPU mirrors after all z-slots.
    pub(crate) fn restore_versions(&self, encoder: &mut wgpu::CommandEncoder) {
        for (index, version) in self.versions.images.iter().enumerate() {
            // Later records for this format/layer supersede earlier incarnations.
            if !self.versions.images[index + 1..]
                .iter()
                .any(|v| (v.stamp & 0xff01) == (version.stamp & 0xff01))
            {
                self.copy_image(encoder, version);
            }
        }
    }

    /// Copies full aligned rows into an existing physical atlas layer.
    fn copy_image(&self, encoder: &mut wgpu::CommandEncoder, version: &Version) {
        let color = version.stamp & 1 != 0;
        // The full-page pitch is 2048 or 8192 bytes, both divisible by 256.
        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer: &version.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(PAGE * if color { 4 } else { 1 }),
                    rows_per_image: Some(PAGE),
                },
            },
            wgpu::TexelCopyTextureInfo {
                texture: if color { &self.color } else { &self.mask },
                // Layer coordinates are separate from the incarnation in the upper word.
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: (version.stamp >> 8) & 0xff,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: PAGE,
                height: PAGE,
                depth_or_array_layers: 1,
            },
        );
    }
}

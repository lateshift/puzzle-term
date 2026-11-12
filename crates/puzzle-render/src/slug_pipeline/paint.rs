//! Paint texture row uploads and geometric growth (design §9.5, §9.8).
//!
//! Addresses stay stable when a new texture copies the old rows. The caller
//! rebinds its view before drawing; device restoration uses the CPU mirror.
//!
//! wgpu texture sizes are immutable. Growth allocates another float texture,
//! submits an old-to-new copy, then replaces the view held by the bind group.
//! Queue ordering guarantees this copy precedes the new rows and later draw.
//! No packed paint index changes during growth: its x/y address stays stable.
//! The shader derives that address from a row-major index with width 256.
//! CPU compaction may reassign indexes; those new rows arrive as frame uploads
//! alongside rebuilt instances. This module never chooses eviction victims.
//!
//! A queue write can address a partial row without 256-byte buffer padding;
//! that padding rule applies to encoded buffer copies, not write_texture.
//! Splitting a linear span at row boundaries preserves arbitrary first texels,
//! including the legacy M3 probe in row zero and full COLR rows after it.
//! The texture includes COPY_SRC even before growth so its old bytes can be
//! copied directly, without a CPU readback or a duplicate GPU upload.
//! Every upload is validated before growth or any write. The frame owner only
//! clears its pending uploads after the entire list has reached the queue.
use super::SlugPipeline;
use crate::RenderError;
/// Allocates a 256-wide float table with copy usage for later growth.
pub(super) fn texture(device: &wgpu::Device, height: u32) -> wgpu::Texture {
    // Nonfilterable Rgba32Float keeps stop offsets and affine coefficients exact.
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Slug COLR paint table"),
        size: wgpu::Extent3d {
            width: 256,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        // COPY_SRC preserves rows when the GPU texture doubles in height.
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}
impl SlugPipeline {
    /// Uploads row-major float texels; true means the caller must rebind views.
    pub fn upload_paint(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        first: u32,
        colors: &[[f32; 4]],
    ) -> Result<bool, RenderError> {
        // Checked sizes prevent hostile frame metadata from wrapping an end index.
        let count = u32::try_from(colors.len()).map_err(|e| RenderError::Upload(e.to_string()))?;
        let end = first
            .checked_add(count)
            .ok_or_else(|| RenderError::Upload("paint span overflow".into()))?;
        // The fixed width and maximum height follow WebGPU default texture limits.
        if end > 256 * 8192 {
            return Err(RenderError::Upload(
                "paint table exceeds texture limit".into(),
            ));
        }
        // An empty span consumes no texture allocation or queue command.
        if count == 0 {
            return Ok(false);
        }
        // A complete last partial row still needs physical texture storage.
        let required = end.div_ceil(256).next_power_of_two();
        // Powers of two amortize growth while keeping the CPU row addresses stable.
        let grew = required > self.paint.height();
        if grew {
            self.grow_paint(device, queue, required);
        }
        // A contiguous span may start midway through a row. Separate queue
        // writes avoid padding or letting an upload cross a physical row.
        let mut offset = 0usize;
        while offset < colors.len() {
            // The first uploaded texel may be inside, rather than at the start of, a row.
            let index = first + offset as u32;
            // Stop at the current row edge; the following iteration resumes at x=0.
            let len = (256 - index % 256).min(count - offset as u32) as usize;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.paint,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: index % 256,
                        y: index / 256,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&colors[offset..offset + len]),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(len as u32 * 16),
                    rows_per_image: Some(1),
                },
                wgpu::Extent3d {
                    width: len as u32,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            // Advancing by texels, not bytes, keeps the caller's linear address contract.
            offset += len;
        }
        Ok(grew)
    }
    /// Copies all old rows before publishing the replacement view.
    fn grow_paint(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, height: u32) {
        // The replacement includes every old row, even currently unused reserved rows.
        // Only texture storage grows; instance paint addresses remain unchanged.
        let next = texture(device, height);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grow paint table"),
        });
        // Exact same-format copying preserves previously uploaded gradient records.
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.paint,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &next,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            self.paint.size(),
        );
        // The copy submission precedes subsequent writes and draws on this queue.
        queue.submit([encoder.finish()]);
        // Publish the texture and its view together; callers rebind before drawing.
        self.paint_view = next.create_view(&wgpu::TextureViewDescriptor::default());
        self.paint = next;
    }
}

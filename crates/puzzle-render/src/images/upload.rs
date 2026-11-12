//! Cold image texture creation and retained instance uploads use the existing raster ABI.
//!
//! Large sources can exceed the device's maximum texture dimension even though
//! they fit the image allowance. Tiles retain their original source stride and
//! a one-texel halo, so scaling across a seam samples the same four pixels as an
//! untiled image. Halos are charged to residency along with the visible texels.
//! Texture bindings own the uploaded handles; source bytes remain in snapshots,
//! not this GPU owner. That distinction permits LRU eviction and device recovery.
use super::*;
/// Creates GPU resources only after the first actually visible image publication.
impl ImageCache {
    /// Allocates only tiny common bindings; source textures wait for visible placements.
    pub(crate) fn new(renderer: &Renderer) -> Self {
        // Bind the surface projection independently of whichever text context is lent.
        let device = renderer.device();
        // Preserve device-pixel origins when an old frame meets a differently sized drawable.
        let viewport = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("image viewport"),
            // vec2 viewport plus vec2 padding satisfies uniform-buffer alignment.
            size: 16,
            // COPY_DST permits surface-resize updates without remapping GPU memory.
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            // Queue writes own all transfers; no mapped memory survives a frame.
            mapped_at_creation: false,
        });
        // Common raster bindings require a mask and paint table, but images use neither.
        // One texel satisfies that ABI without allocating an emoji atlas page.
        let mask = texture(device, [1, 1], wgpu::TextureFormat::R8Unorm);
        let paint = texture(device, [1, 1], wgpu::TextureFormat::Rgba32Float)
            .create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            // wgpu handle identity detects replacement after device loss.
            device: device.clone(),
            viewport,
            // Structurally identical layouts make bind groups pipeline-compatible.
            layout: crate::raster_pipeline::setup::bindings_layout(device),
            mask,
            paint,
            // Images use textureLoad for alpha-correct filtering; this ABI slot is inert.
            sampler: device.create_sampler(&wgpu::SamplerDescriptor::default()),
            // Collections stay empty until image admission and then keep their capacities.
            entries: Vec::new(),
            bytes: 0,
            clock: 0,
            quads: Vec::new(),
            draws: Vec::new(),
            // Start at one record; visible placements trigger geometric growth later.
            instances: buffer(device, 1),
            capacity: 1,
        }
    }
    /// Neighbor borders preserve premultiplied bilinear sampling across hardware tiles.
    pub(super) fn upload(
        &self,
        renderer: &Renderer,
        source: &ImageSource,
    ) -> Result<Entry, RenderError> {
        // ImageSource proved the entire row-major RGBA slice is present.
        let size = source.size();
        let limit = renderer.device().limits().max_texture_dimension_2d;
        // RGBA8 arithmetic is checked before passing a byte stride to the driver.
        let stride = size[0]
            .checked_mul(4)
            .ok_or_else(|| RenderError::Upload("image stride overflow".into()))?;
        let mut tiles = Vec::new();
        // Reserve one neighbor on each interior edge. The actual hardware limit,
        // rather than a workload threshold, determines how many tiles are needed.
        let step = puzzle_frame::image_tile_step(size, limit);
        // Each logical source pixel belongs to exactly one destination quad.
        for y in (0..size[1]).step_by(step[1] as usize) {
            for x in (0..size[0]).step_by(step[0] as usize) {
                // Last tiles truncate at the decoded image boundary, not a padded page.
                let rect = [x, y, step[0].min(size[0] - x), step[1].min(size[1] - y)];
                // Draw only the logical tile; its upload also contains neighboring ink.
                let upload = puzzle_frame::image_tile_border(rect, size);
                let color = texture(
                    renderer.device(),
                    [upload[2], upload[3]],
                    wgpu::TextureFormat::Rgba8Unorm,
                );
                // Start each row upload at its halo origin inside the shared CPU source.
                let offset = (upload[1] as usize * size[0] as usize + upload[0] as usize) * 4;
                // Original row stride uploads the halo without allocating a tile copy.
                renderer.queue.write_texture(
                    color.as_image_copy(),
                    &source.pixels()[offset..],
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(stride),
                        // One layer consumes exactly this many source rows.
                        rows_per_image: Some(upload[3]),
                    },
                    wgpu::Extent3d {
                        width: upload[2],
                        height: upload[3],
                        // Every independent tile has one RGBA layer.
                        depth_or_array_layers: 1,
                    },
                );
                // Explicit layouts let this binding use the already compiled raster shader.
                let binding = crate::raster_pipeline::pages::bindings(
                    renderer.device(),
                    &self.layout,
                    &self.viewport,
                    // Glyph-only bindings reference the tiny shared stand-ins.
                    &self.mask,
                    &color,
                    &self.sampler,
                    &self.paint,
                );
                tiles.push(Tile {
                    rect,
                    upload,
                    binding,
                });
            }
        }
        // Keep only a Weak allocation identity: deleting CPU images frees their bytes.
        // Actual texture lifetime follows the bind groups and in-flight queue commands.
        Ok(Entry {
            pixels: std::sync::Arc::downgrade(source.pixels()),
            size,
            // Charge duplicated neighbor texels as real GPU residency bytes.
            bytes: puzzle_frame::image_resident_bytes(size, limit),
            // A successful visible upload is the most recent use in this surface.
            used: self.clock,
            tiles,
        })
    }
    /// Reuses the forty-byte instance allocation and one projection buffer per surface.
    pub(super) fn write_instances(&mut self, renderer: &Renderer, target: [u32; 2]) {
        // Tile expansion can add quads, but warm geometry reuses this vertex allocation.
        let count = self.quads.len() as u32;
        // Power-of-two growth bounds reallocations while preserving the raster ABI.
        if count > self.capacity {
            self.capacity = count.next_power_of_two();
            self.instances = buffer(renderer.device(), self.capacity);
        }
        // The acquired texture may differ from the published frame after resize.
        let viewport = [target[0].max(1) as f32, target[1].max(1) as f32, 0.0, 0.0];
        renderer
            .queue
            .write_buffer(&self.viewport, 0, bytemuck::cast_slice(&viewport));
        // Zero-image frames still update projection but issue no empty vertex write.
        if !self.quads.is_empty() {
            renderer
                .queue
                .write_buffer(&self.instances, 0, bytemuck::cast_slice(&self.quads));
        }
    }
}
/// Exact-sized sampled source texture; no glyph page or CPU mirror is allocated here.
fn texture(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::Texture {
    // There is one base mip: terminal images use bilinear filtering, not mip generation.
    // COPY_DST enables one-time upload; source sampling never reads back to the CPU.
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("independent image texture"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        // No mip chain means byte accounting equals the uploaded base-level area.
        mip_level_count: 1,
        // Raster images need no multisample resolve or secondary attachment.
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        // Textures are sampled only; all image decoding happened before publication.
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}
/// Source geometry has its own persistent vertex buffer, independent of text contexts.
fn buffer(device: &wgpu::Device, capacity: u32) -> wgpu::Buffer {
    // Forty bytes per record is pinned by puzzle-frame; no private vertex ABI exists.
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("image raster instances"),
        // Widen before multiplication so vertex-buffer byte sizing cannot wrap u32.
        size: u64::from(capacity) * 40,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

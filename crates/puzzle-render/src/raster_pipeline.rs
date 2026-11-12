//! Atlas draw state for bitmap glyphs and color images (§10.3, D-28).
//!
//! The browser cluster source supplies crops through the neutral contract to
//! R8 coverage page and RGBA8 color page. Future rasterizers can upload
//! pixels into them without changing frame order or text placement.
//! A nearest sampler preserves one coverage texel per grid device pixel.
//!
//! The two pages have different meanings. The mask page contains one
//! channel of coverage; the shader applies each instance's text color.
//! The color page carries its own red, green, blue and alpha channels.
//! Separating them lets a monochrome glyph use one byte per texel while
//! color emoji retains its source pixels without an external color pass.
//!
//! Arrays grow by whole pages within the configured allowance. At the 48-MiB
//! default, web uses four masks/two colors and native uses eight masks/one color. Uploads name a rectangle
//! and provide tightly packed rows, so a new glyph changes only its own
//! small region. The application never reads atlas pixels back to tint
//! text, avoiding a GPU-to-CPU stall on the path to the next display frame.
//! The frame builder decides glyph origins, advances and z-order; this
//! module only executes the already-chosen placement (D-28).
//!
//! The atlas dimensions are 2048×2048 per §10.3. Browser fallback supplies
//! complete cluster samples, while synthetic pipeline tests exercise both
//! formats and verify that texture growth preserves previous pages.
//! The CPU shelf allocator owns eviction and generation policy. Texture arrays
//! need not change how layout maps clusters onto terminal cells.
//!
//! Each instance carries both destination rectangle and source UV bounds.
//! One forty-byte upload can therefore cover all bitmap glyphs in a frame,
//! even when text and cursor batches occupy different z-slots. A single
//! nearest sampler keeps one hinted coverage texel aligned to one device
//! pixel; transformed bitmap-only ink uses premultiplied bilinear sampling.
//!
//! WebGPU normalizes R8 and RGBA8 samples to floats. The shader multiplies
//! mask coverage with the instance tint, but color pages already include
//! their own RGB. Both branches output premultiplied alpha, matching the
//! rect and Slug pipelines in the same render pass.
//!
//! The viewport uniform is updated after resize without recompilation.
//! Atlas coordinates do not depend on canvas CSS size or desktop window
//! scale. The platform supplies the exact device-pixel target dimensions;
//! shader projection and atlas lookup then stay separate operations.
//!
//! Upload validation is important even though the current producer is
//! internal. Browser fallback pixels cross the Canvas ImageData boundary.
//! Rectangle bounds and byte counts are checked before queue writes; an
//! invalid upload returns a report instead of panicking on a slice (R8).
use crate::RenderError;
/// Texture arrays grow independently from the instance buffer.
pub(crate) mod pages;
/// Pipeline declarations are independent of atlas allocation policy.
pub(crate) mod setup;
/// Capacity-only staging copies reuse the existing bounded atlas textures.
mod versions;
use puzzle_frame::{RasterInstance, RectU32, Upload};
use setup::{bindings_layout, compile};

/// Fixed 2048² atlas dimension required by the renderer contract.
const PAGE: u32 = 2048;

/// Persistent atlas textures, shader and growable instance storage.
pub struct RasterPipeline {
    /// One shader handles tinted masks and self-colored pixels.
    pub(crate) pipeline: wgpu::RenderPipeline,
    /// Retains both atlas views and the nearest sampler.
    bind_group: wgpu::BindGroup,
    /// Device-pixel viewport sent to the vertex stage.
    viewport: wgpu::Buffer,
    /// R8 coverage page, owned while the bind group samples it.
    mask: wgpu::Texture,
    /// RGBA8 color page, owned while the bind group samples it.
    color: wgpu::Texture,
    /// Retained declaration needed when array growth replaces views.
    layout: wgpu::BindGroupLayout,
    /// Pixel-aligned text uses one nearest sampler across both arrays.
    sampler: wgpu::Sampler,
    /// Shared paint rows provide effect matrices without growing the instance ABI.
    paint: wgpu::TextureView,
    /// Uploaded forty-byte instances shared by all raster batches.
    instances: wgpu::Buffer,
    /// Power-of-two instance capacity retained between frames.
    capacity: u32,
    /// Combined mask/color byte capacity selected by the shared text owner.
    budget_bytes: usize,
    /// Immutable page images needed by the current frame's ordered draw segments.
    versions: versions::Versions,
}

impl RasterPipeline {
    /// Creates both atlas pages and compiles one target-format pipeline.
    #[must_use]
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        paint: &wgpu::TextureView,
    ) -> Self {
        Self::shared(device, format, paint, None)
    }
    /// Reuses an immutable shader while keeping sampled atlas pages context-local.
    pub(crate) fn shared(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        paint: &wgpu::TextureView,
        shared: Option<&wgpu::RenderPipeline>,
    ) -> Self {
        // Bind empty but valid textures immediately. Shader validation can
        // then catch a format mismatch before any fallback glyph arrives.
        // Views are temporary handles for bind-group creation; retaining
        // the texture objects below keeps their underlying storage alive.
        let mask = pages::atlas(device, wgpu::TextureFormat::R8Unorm, 1);
        let color = pages::atlas(device, wgpu::TextureFormat::Rgba8Unorm, 1);
        // Nearest sampling preserves a one-pixel hinted stem. Bilinear
        // filtering here would blur text that the CPU carefully aligned.
        // Both pages share the sampler because the shader selects one at
        // a time and both use normalized texture coordinates.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nearest raster glyph sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..wgpu::SamplerDescriptor::default()
        });
        // WGSL's two-float viewport occupies a 16-byte uniform record;
        // the extra bytes are explicit padding, not terminal geometry.
        // Resizing rewrites this buffer without invalidating the pipeline.
        let viewport = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("raster viewport"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Binding visibility is narrow: projection belongs to vertices,
        // and the atlas and sampler are consumed only by fragments.
        // One bind group is reused by every raster batch in a frame.
        let layout = bindings_layout(device);
        let bind_group =
            pages::bindings(device, &layout, &viewport, &mask, &color, &sampler, paint);
        // The attachment format may differ on Metal and browser WebGPU;
        // the platform passes its actual format into this constructor.
        Self {
            pipeline: shared
                .cloned()
                .unwrap_or_else(|| Self::compile_shared(device, format)),
            bind_group,
            viewport,
            mask,
            color,
            instances: instance_buffer(device, 1),
            capacity: 1,
            budget_bytes: puzzle_slug::atlas::DEFAULT_BUDGET_BYTES,
            layout,
            sampler,
            paint: paint.clone(),
            versions: versions::Versions::default(),
        }
    }

    /// Compiles format-bound state without allocating any raster pages.
    pub(crate) fn compile_shared(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        compile(device, &bindings_layout(device), format)
    }

    /// Refreshes the shared effect rows when Slug grows its paint texture.
    pub(crate) fn set_paint_view(&mut self, device: &wgpu::Device, paint: &wgpu::TextureView) {
        // Atlas-array growth later reuses this current shared view too.
        self.paint = paint.clone();
        self.bind_group = pages::bindings(
            device,
            &self.layout,
            &self.viewport,
            &self.mask,
            &self.color,
            &self.sampler,
            &self.paint,
        );
    }

    /// Updates target dimensions without invalidating atlas bindings.
    pub fn set_viewport(&self, queue: &wgpu::Queue, width: u32, height: u32) {
        // Minimized surfaces may briefly report zero. Platforms skip
        // drawing them, but the clamp also protects the shader division.
        let values = [width.max(1) as f32, height.max(1) as f32, 0.0, 0.0];
        queue.write_buffer(&self.viewport, 0, bytemuck::cast_slice(&values));
    }

    /// Copies forty-byte instances once for all raster z-slots.
    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, data: &[RasterInstance]) {
        // One contiguous array serves all ordered z-slots. Per-row writes
        // would turn a full redraw into many small queue submissions.
        // Doubling makes occasional capacity growth cheap over time.
        let count = data.len() as u32;
        if count > self.capacity {
            self.capacity = count.next_power_of_two();
            self.instances = instance_buffer(device, self.capacity);
        }
        if !data.is_empty() {
            // Pod casting uses the tested repr(C) byte layout; no mapped
            // pointer or handwritten unsafe conversion is needed (R8).
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(data));
        }
    }

    /// Uploads a validated one-page rectangle before a matching batch.
    pub fn apply_upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        upload: &Upload,
    ) -> Result<(), RenderError> {
        // Version images become copy buffers, not extra sampled atlas layers.
        if matches!(upload, Upload::RasterReset | Upload::RasterPage { .. }) {
            return self.versions.apply(device, queue, upload);
        }
        let (page, rect, pixels, color) = match upload {
            Upload::Mask { page, rect, pixels } => (*page, *rect, pixels, false),
            Upload::Color { page, rect, pixels } => (*page, *rect, pixels, true),
            _ => return Ok(()),
        };
        self.ensure_page(device, queue, page, color)?;
        let texture = if color { &self.color } else { &self.mask };
        write_pixels(
            queue,
            texture,
            page,
            rect,
            pixels,
            if color { 4 } else { 1 },
        )
    }

    /// Counts actual sampled texture capacity, including initially empty layers.
    pub(crate) fn allocated_bytes(&self) -> usize {
        PAGE as usize
            * PAGE as usize
            * (self.mask.size().depth_or_array_layers as usize
                + self.color.size().depth_or_array_layers as usize * 4)
    }

    /// Applies policy to a fresh pipeline before any non-default page upload.
    pub(crate) fn set_budget(&mut self, bytes: usize) {
        self.budget_bytes = bytes;
    }

    /// Grows either sampled array only within the unchanged combined raster allowance.
    pub(super) fn ensure_page(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        page: u32,
        color: bool,
    ) -> Result<(), RenderError> {
        // Page indices have eight ABI bits and must fit the adapter's array limit.
        if page >= 256.min(device.limits().max_texture_array_layers) {
            return Err(RenderError::Upload(
                "raster array layer limit exceeded".into(),
            ));
        }
        // Exact whole-page growth respects non-power-of-two configured budgets.
        let masks = self
            .mask
            .size()
            .depth_or_array_layers
            .max(if color { 1 } else { page + 1 });
        let colors = self
            .color
            .size()
            .depth_or_array_layers
            .max(if color { page + 1 } else { 1 });
        let bytes = super::raster_pipeline::PAGE as usize
            * super::raster_pipeline::PAGE as usize
            * (masks as usize + colors as usize * 4);
        if bytes > self.budget_bytes {
            return Err(RenderError::Upload(
                "combined raster capacity exceeds configured budget".into(),
            ));
        }
        let texture = if color {
            &mut self.color
        } else {
            &mut self.mask
        };
        if pages::grow(device, queue, texture, page + 1) {
            self.bind_group = pages::bindings(
                device,
                &self.layout,
                &self.viewport,
                &self.mask,
                &self.color,
                &self.sampler,
                &self.paint,
            );
        }
        Ok(())
    }

    /// Draws one batch in the caller's existing ordered render pass.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, range: std::ops::Range<u32>) {
        // Empty bitmap batches cost no draw call while M6 has no producer.
        // Later text fallback can use this path without a new render pass.
        if range.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        // Six shader vertices expand one instance to two triangles; batch
        // ranges choose the already-ordered rows without copying them.
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.draw(0..6, range);
    }
}

/// Writes one bounded atlas rectangle with an exact source byte count.
fn write_pixels(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    page: u32,
    rect: RectU32,
    pixels: &[u8],
    channels: u32,
) -> Result<(), RenderError> {
    // Browser-supplied fallback imagery is untrusted at this boundary.
    // Checked arithmetic rejects wraparound before writing GPU memory.
    let right = rect
        .x
        .checked_add(rect.width)
        .ok_or_else(|| RenderError::Upload("atlas width overflow".into()))?;
    let bottom = rect
        .y
        .checked_add(rect.height)
        .ok_or_else(|| RenderError::Upload("atlas height overflow".into()))?;
    let bytes = rect
        .width
        .checked_mul(rect.height)
        .and_then(|area| area.checked_mul(channels))
        .ok_or_else(|| RenderError::Upload("atlas upload size overflow".into()))?;
    if right > PAGE || bottom > PAGE || usize::try_from(bytes).ok() != Some(pixels.len()) {
        return Err(RenderError::Upload(
            "atlas upload exceeds page or byte count".into(),
        ));
    }
    if bytes == 0 {
        return Ok(());
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: rect.x,
                y: rect.y,
                z: page,
            },
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(rect.width * channels),
            rows_per_image: Some(rect.height),
        },
        wgpu::Extent3d {
            width: rect.width,
            height: rect.height,
            depth_or_array_layers: 1,
        },
    );
    Ok(())
}

/// Allocates a retained vertex copy destination for future atlas producers.
fn instance_buffer(device: &wgpu::Device, capacity: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("raster instances"),
        size: u64::from(capacity) * 40,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// GPU-only crop copies and pre-reuse captures use the same physical sampled arrays.
mod gpu;

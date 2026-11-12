//! Instanced Slug draw pipeline, using the licensed WGSL reference port.
//!
//! The 56-byte Rust instance is expanded by the vertex shader into the
//! reference's five float4 attributes. The pixel shader reads the exact
//! Rgba32Float curve and Rg16Uint band planes prepared by puzzle-slug.
//! One immutable pipeline and index buffer serve any number of glyphs;
//! the instance buffer doubles only when a larger batch arrives.
//!
//! The paint texture has one 256-entry solid-color row in M3. Index zero
//! selects the instance's packed color; nonzero indexes select a row texel.
//! Later gradient records can grow that table without changing the shape
//! instance or the pixel coverage port.
//!
//! Texture binding types matter here. Rgba32Float is unfilterable, so its
//! binding declares a nonfilterable float sample type; Rg16Uint declares
//! unsigned integers. No sampler appears in the WGSL or bind group, because
//! a filtered control point or band offset would corrupt outline geometry.
//! The viewport uniform is visible to the vertex stage, where dynamic
//! dilation projects a corner into clip space. The fragment stage receives
//! the resulting em coordinate and flat band metadata through varyings.
//!
//! An instance is not a ready-made GPU quad. Four static vertex indexes
//! choose its bbox corners, and six static triangle indexes connect them.
//! The pipeline therefore uploads one 56-byte record per shape instead of
//! four copies of the reference's five float4 vertex attributes. This
//! reduces upload volume when text changes without altering shader math.
use puzzle_frame::slug::SlugInstance;
use wgpu::util::DeviceExt;

/// GPU objects required to draw a batch of Slug instances into one pass.
///
/// A caller can retain this value across frames. Updating the viewport or
/// instance slice writes existing buffers; only a larger batch allocates
/// another instance buffer. Texture growth requires `rebind` because a view
/// refers to one immutable GPU allocation, not to a movable atlas address.
pub struct SlugPipeline {
    /// Compiled vertex and pixel stages for one target format.
    pub(crate) pipeline: wgpu::RenderPipeline,
    /// Current curve, band and paint texture views plus viewport uniform.
    bind_group: wgpu::BindGroup,
    /// Layout is retained so growth can rebind new texture views.
    bind_layout: wgpu::BindGroupLayout,
    /// Four MVP rows plus viewport dimensions, 80 bytes total.
    uniform: wgpu::Buffer,
    /// Two triangles use four corners emitted from vertex_index 0–3.
    index: wgpu::Buffer,
    /// Contiguous plain instance bytes for one Slug batch.
    instances: wgpu::Buffer,
    /// Buffer capacity in instances; growth doubles to avoid per-frame churn.
    capacity: u32,
    /// Solid-paint row reserved for nonzero paint indexes.
    paint: wgpu::Texture,
    /// View retained while re-creating a bind group after atlas growth.
    paint_view: wgpu::TextureView,
}

/// Float paint texture growth and row-major uploads.
mod paint;
/// Builds bind layouts and compiles the shader for one target format.
mod setup;
use setup::{bindings, layout, pipeline};

/// One upload and indexed draw for every contiguous Slug batch.
///
/// Separating `upload` from `draw` lets the caller finish all queue writes
/// before opening a render pass. This avoids a per-glyph submission and
/// leaves pass ordering to the frame builder's z-slot batches (§10.3).
impl SlugPipeline {
    /// Shares paint metadata with the raster effect vertex stage (§8.9).
    pub(crate) fn paint_view(&self) -> &wgpu::TextureView {
        &self.paint_view
    }

    /// Compiles the shader for a target format and binds the current atlas.
    ///
    /// The caller has already created the device and texture planes. This
    /// constructor owns neither platform handles nor the CPU atlas, keeping
    /// puzzle-render reusable in a canvas and the native harness (R9).
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        curves: &wgpu::TextureView,
        bands: &wgpu::TextureView,
    ) -> Self {
        Self::new_with(device, format, curves, bands, pipeline)
    }

    /// Shares immutable shader state; this context still owns its atlas and paint bindings.
    pub(crate) fn shared(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        curves: &wgpu::TextureView,
        bands: &wgpu::TextureView,
        shared: &wgpu::RenderPipeline,
    ) -> Self {
        Self::new_with(device, format, curves, bands, |_, _, _| shared.clone())
    }
    /// Compiles without allocating a context's texture, paint or instance buffers.
    pub(crate) fn compile_shared(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        pipeline(device, &layout(device), format)
    }

    /// Builds the same Slug vertex stage with a test-only float input output.
    #[cfg(feature = "oracle-debug")]
    pub fn new_debug(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        curves: &wgpu::TextureView,
        bands: &wgpu::TextureView,
    ) -> Self {
        Self::new_with(device, format, curves, bands, setup::debug_pipeline)
    }

    /// Shares texture, buffer and instance setup across fragment entry points.
    fn new_with(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        curves: &wgpu::TextureView,
        bands: &wgpu::TextureView,
        compile: impl FnOnce(
            &wgpu::Device,
            &wgpu::BindGroupLayout,
            wgpu::TextureFormat,
        ) -> wgpu::RenderPipeline,
    ) -> Self {
        // The bind layout is created before the pipeline so shader resource
        // compatibility is checked once against the declared bindings.
        let bind_layout = layout(device);
        // Four clip-space rows and one viewport row are 5×16 bytes. Uniform
        // writes change the projection without recompiling Slug's shader.
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Slug MVP and viewport"),
            size: 80,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // This solid-color table is a binding-compatible stub for the paint
        // records added by later milestones; index zero bypasses it.
        let paint = paint::texture(device, 2);
        // The view is retained separately because a bind group holds it;
        // later solid entries can be uploaded to the texture by index.
        let paint_view = paint.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = bindings(device, &bind_layout, &uniform, curves, bands, &paint_view);
        // The quad index order gives two triangles with a shared diagonal.
        // All glyphs use the same six u16 values, regardless of outline size.
        let index = quad_index(device);
        // One instance is enough for an empty or first-glyph scene; doubling
        // later keeps ordinary frames from allocating a new GPU buffer.
        let instances = instance_buffer(device, 1);
        Self {
            pipeline: compile(device, &bind_layout, format),
            bind_group,
            bind_layout,
            uniform,
            index,
            instances,
            capacity: 1,
            paint,
            paint_view,
        }
    }

    /// Rebinds texture views only after one atlas plane has grown.
    ///
    /// Atlas growth preserves packed coordinates but changes GPU texture
    /// objects. The old bind group still owns old views, so a new one is
    /// required even though no SlugInstance bytes need alteration.
    pub fn rebind(
        &mut self,
        device: &wgpu::Device,
        curves: &wgpu::TextureView,
        bands: &wgpu::TextureView,
    ) {
        self.bind_group = bindings(
            device,
            &self.bind_layout,
            &self.uniform,
            curves,
            bands,
            &self.paint_view,
        );
    }

    /// Sets an orthographic viewport whose object space uses device pixels.
    ///
    /// The reference MVP is four rows. Top-left device pixels map to clip
    /// x=-1 and y=+1; positive screen y maps downward. The viewport row
    /// supplies physical dimensions to the dilation formula, which expands
    /// edges by roughly half a device pixel after projection.
    pub fn set_viewport(&self, queue: &wgpu::Queue, width: u32, height: u32) {
        let w = width.max(1) as f32;
        let h = height.max(1) as f32;
        // Row vectors map top-left device pixels to WebGPU clip coordinates.
        // The fourth row fixes homogeneous w=1 for the reference dilation.
        let rows: [f32; 20] = [
            2.0 / w,
            0.0,
            0.0,
            -1.0,
            0.0,
            -2.0 / h,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            w,
            h,
            0.0,
            0.0,
        ];
        // This is a queued GPU write, so the next submitted pass observes
        // the new matrix without waiting for a CPU readback or a fence.
        queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&rows));
    }

    /// Uploads an instance slice, growing the GPU buffer by powers of two.
    ///
    /// Reusing capacity is essential for terminal frames where the number
    /// of visible glyphs stays nearly constant while their content changes.
    /// No outline data is copied here; only transforms and atlas handles.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[SlugInstance],
    ) {
        let count = instances.len() as u32;
        if count > self.capacity {
            // A power-of-two allocation amortizes growth over many frames.
            // All byte offsets remain within the 56-byte Rust/WGSL contract.
            self.capacity = count.next_power_of_two();
            self.instances = instance_buffer(device, self.capacity);
        }
        if !instances.is_empty() {
            // Pod casting is safe for this repr(C) struct and avoids a
            // hand-written byte serializer in the frame hot path (R8).
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(instances));
        }
    }

    /// Draws the uploaded instances in one pass after the target is cleared.
    ///
    /// The caller controls z-order and scissor state. This method only binds
    /// Slug resources and emits an indexed draw, so overlay and terminal
    /// batches can share a render pass without a hidden clear operation.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, count: u32) {
        self.draw_range(pass, 0..count);
    }

    /// Draws one ordered frame batch from the shared instance upload.
    pub fn draw_range(&self, pass: &mut wgpu::RenderPass<'_>, range: std::ops::Range<u32>) {
        // A frame can place text in several z-slots without copying its
        // prepared instances into a new buffer for each draw call.
        if range.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.set_index_buffer(self.index.slice(..), wgpu::IndexFormat::Uint16);
        // Instance zero begins at the start of the uploaded contiguous slice;
        // later frame batches can add an offset when their contract arrives.
        pass.draw_indexed(0..6, 0, range);
    }

    /// The reserved paint table remains owned while its bind group exists.
    pub fn paint_texture(&self) -> &wgpu::Texture {
        &self.paint
    }

    /// Uploads the first solid paint-table texel for the M3 binding probe.
    pub fn set_probe_paint(&self, queue: &wgpu::Queue, rgba: [f32; 4]) {
        // A nonzero instance paint index fetches this exact RGBA32Float
        // texel. The zero index still uses the instance's packed color.
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.paint,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::bytes_of(&rgba),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(16),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }
}

/// Creates the immutable two-triangle index order for each expanded quad.
fn quad_index(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Slug quad index"),
        contents: bytemuck::cast_slice(&[0u16, 1, 2, 2, 1, 3]),
        usage: wgpu::BufferUsages::INDEX,
    })
}

/// Reserves bytes for instance uploads without initializing a mapped buffer.
///
/// Vertex data comes from queue writes rather than a mapped CPU pointer;
/// that keeps the backing storage owned by wgpu and obeys R8's safety rule.
fn instance_buffer(device: &wgpu::Device, capacity: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Slug instance batch"),
        size: u64::from(capacity) * 56,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

//! Persistent instanced rectangle pipeline for terminal fills (§10.3).
//!
//! Rectangles share one shader and instance buffer regardless of how many
//! adjacent cells a background run spans. The buffer doubles only when a
//! larger frame appears, keeping steady frames free of GPU reallocations.
//! The shader consumes the frame contract's exact 32-byte layout; its
//! pixel coordinates are independent of the platform surface API (R9).
//! A viewport uniform is retained so a resize writes sixteen bytes rather
//! than recompiling the shader or rewriting all rectangle positions.
//! Background runs, straight underlines, selection and cursor geometry
//! share this state. That keeps their painter order in frame batches, not
//! in separate render passes with repeated clears.
//! Alpha is premultiplied in WGSL because the shared pass blends shaped
//! Slug ink and rectangles over the same target (§10.3).
use puzzle_frame::RectInstance;

/// A retained shader, viewport binding and geometrically grown instance array.
pub struct RectPipeline {
    /// Validated WGSL draw state for one target format.
    pub(crate) pipeline: wgpu::RenderPipeline,
    /// Bound viewport uniform shared by all rectangles in a pass.
    bind_group: wgpu::BindGroup,
    /// Sixteen-byte viewport uniform, padded for WebGPU layout.
    viewport: wgpu::Buffer,
    /// GPU copy of the frame's rect instance slice.
    instances: wgpu::Buffer,
    /// Number of instances the retained buffer can hold.
    capacity: u32,
}

impl RectPipeline {
    /// Compiles the rect shader once for a surface or offscreen format.
    #[must_use]
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self::shared(device, format, None)
    }

    /// Reuses a process pipeline while retaining independent atlas/instance bindings.
    pub(crate) fn shared(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        shared: Option<&wgpu::RenderPipeline>,
    ) -> Self {
        let layout = layout(device);
        // WebGPU uniforms require a 16-byte allocation for two floats;
        // padding is explicit in the matching WGSL Viewport struct.
        let viewport = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rect viewport"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // The bind group stays valid across target resizes because the
        // uniform buffer object stays fixed while its contents change.
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rect viewport binding"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport.as_entire_binding(),
            }],
        });
        // Surface formats vary between Metal and browser WebGPU; compile
        // for the actual attachment format chosen by the platform shell.
        let pipeline = shared
            .cloned()
            .unwrap_or_else(|| Self::compile_shared(device, format));
        Self {
            pipeline,
            bind_group,
            viewport,
            instances: instance_buffer(device, 1),
            capacity: 1,
        }
    }

    /// Compiles format-bound shader state without allocating window or atlas storage.
    pub(crate) fn compile_shared(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        // Validation happens at construction, before a frame can enter
        // its hot path; malformed WGSL is an implementation bug.
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("terminal rect WGSL"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/rect.wgsl").into()),
        });
        // Only the vertex stage reads the viewport; fragment coverage
        // depends on interpolated local device-pixel distances instead.
        compile(device, &shader, &layout(device), format)
    }

    /// Maps device-pixel coordinates to the current render target.
    pub fn set_viewport(&self, queue: &wgpu::Queue, width: u32, height: u32) {
        // A minimized target may report zero; the platform skips drawing,
        // and this clamp keeps the shader's division finite if called.
        let values = [width.max(1) as f32, height.max(1) as f32, 0.0, 0.0];
        queue.write_buffer(&self.viewport, 0, bytemuck::cast_slice(&values));
    }

    /// Copies all rects once, growing capacity by powers of two if needed.
    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, rects: &[RectInstance]) {
        // Instance ranges in batches index this one contiguous upload.
        // Splitting writes by row would create avoidable queue overhead.
        let count = rects.len() as u32;
        if count > self.capacity {
            // A screen redraw can add many backgrounds at once; doubling
            // amortizes subsequent frames without guessing terminal size.
            self.capacity = count.next_power_of_two();
            self.instances = instance_buffer(device, self.capacity);
        }
        if !rects.is_empty() {
            // repr(C) plus Pod fixes the exact bytes without handwritten
            // casts or an unsafe mapped-buffer path (R8).
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(rects));
        }
    }

    /// Emits one contiguous batch inside the frame's existing render pass.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, instances: std::ops::Range<u32>) {
        // Empty batches should not become driver submissions; they occur
        // naturally when a frame has no selection or no cursor ink.
        if instances.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        // Every instance expands into the same six-vertex quad in WGSL.
        // The range selects rows already ordered by the frame builder.
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.draw(0..6, instances);
    }
}

/// Fixes the shader entry points and exact 32-byte instance offsets.
fn compile(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    // A single bind group avoids per-batch descriptor churn. Every rect
    // uses the same viewport even when batch scissors differ.
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("rect pipeline layout"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    // Attribute offsets follow 16 bytes of geometry, then the packed
    // color/radius/stroke/flag words at offsets 16, 20, 24 and 28.
    const ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
        0 => Float32x4, 1 => Uint32, 2 => Float32, 3 => Float32, 4 => Uint32
    ];
    // No depth buffer or MSAA is needed: painter order is explicit and
    // rounded edges use the fragment SDF's one-device-pixel coverage.
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("terminal rect pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("rect_vertex"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: 32,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &ATTRIBUTES,
            })],
        },
        // Triangle-list vertices form two copies of the quad diagonal;
        // shared interpolants make that seam visually continuous.
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("rect_fragment"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            // Slug outputs premultiplied alpha in the same pass. Matching
            // blend factors avoids a color shift at text/background edges.
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Allocates a copy destination so instance bytes stay owned by wgpu.
fn instance_buffer(device: &wgpu::Device, capacity: u32) -> wgpu::Buffer {
    // Queue writes can replace the contents while the object persists;
    // no CPU mapping or readback belongs in terminal frame assembly.
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("terminal rect instances"),
        size: u64::from(capacity) * 32,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Explicitly equivalent layouts allow one immutable shader to bind each context's data.
fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("rect viewport layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

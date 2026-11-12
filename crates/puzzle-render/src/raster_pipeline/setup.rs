//! Raster binding and shader layouts mirror the forty-byte contract (§10.3).
//!
//! A single bind group serves every glyph in both bitmap formats. Array layers
//! select pages without splitting the text into one draw per page or glyph.
//! The two arrays share sampling rules but retain independent layer counts.
//! Their layout remains stable when growth replaces one of the bound textures.
//! Mask coverage and authored RGBA both use filterable normalized channels.
//! Device-aligned glyphs use the nearest sampler created by the page owner.
//! The vertex stream stores only instances; vertex_index generates quad corners.
//! Rectangles and UV boxes each occupy sixteen bytes, followed by two words.
//! No per-vertex geometry allocation is needed when a cached row is assembled.
//! Tint is meaningful for coverage only; color samples keep the font palette.
//! Fragment output premultiplies either branch before the common blend stage.
//! The target format comes from the surface, preserving its gamma-space policy.
//! No optional GPU feature is needed for coverage or authored color sampling.
//! Binding declarations must match raster.wgsl on Metal and browser WebGPU.
//! Effects keep the original vertex ABI: the flag redirects tint to a paint row.
//! The paint texture is unfilterable because vertex metadata must remain exact.
/// Describes one uniform, two floating textures and their sampler.
pub(crate) fn bindings_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    // The binding sequence mirrors the WGSL declarations exactly.
    // Resource incompatibility is detected when the pipeline is built.
    let uniform = wgpu::BindGroupLayoutEntry {
        binding: 0,
        // Device-pixel projection is needed only while expanding quad vertices.
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            // Each painter owns one projection buffer, with no per-draw offset.
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let sampled = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            // Even a one-page atlas uses an array view, so growth keeps this ABI.
            view_dimension: wgpu::TextureViewDimension::D2Array,
            // Atlas texels are resolved CPU samples, never multisample attachments.
            multisampled: false,
        },
        count: None,
    };
    let sampler = wgpu::BindGroupLayoutEntry {
        binding: 3,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    };
    // Affine bitmap metadata is loaded exactly, never sampled or filtered.
    let paint = wgpu::BindGroupLayoutEntry {
        binding: 4,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("raster atlas layout"),
        // Coverage precedes authored color at the shader's fixed binding numbers.
        entries: &[uniform, sampled(1), sampled(2), sampler, paint],
    })
}

/// Builds the forty-byte vertex layout and premultiplied color target.
pub(super) fn compile(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    // Shader creation is the safe WGSL path on both supported targets.
    // No native shader translator is included in WebGPU-only wasm (§2.6).
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("raster atlas WGSL"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/raster.wgsl").into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("raster pipeline layout"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    /// Packed fields match RasterInstance and the WGSL input locations.
    const ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
        0 => Float32x4, 1 => Float32x4, 2 => Uint32, 3 => Uint32
    ];
    // Color alpha is premultiplied in the fragment stage to match Slug.
    // Painter order comes from frame batches rather than a depth buffer.
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("raster atlas pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("raster_vertex"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                // Each record advances once per glyph, not once per corner.
                array_stride: 40,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &ATTRIBUTES,
            })],
        },
        // The default triangle list consumes the shared six-index quad.
        primitive: wgpu::PrimitiveState::default(),
        // Slot ordering and scissors provide layering without depth ownership.
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("raster_fragment"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
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

//! Slug pipeline binding and shader setup.
//!
//! Resource declarations here match the WGSL port exactly. Floating curve
//! points are unfilterable, integer bands are exact, and the MVP uniform is
//! visible only to the vertex stage. The instance attributes are specified
//! byte-for-byte so a wrong offset cannot silently distort a glyph.
//!
//! Pipeline creation is outside the frame loop. It can validate WGSL and
//! layout types once, then one retained pipeline draws both text glyphs and
//! overlay shapes. The shader's band texture is unsigned because curve
//! locations and counts must survive a fetch without rounding. The curve
//! texture is unfilterable float because averaging adjacent control points
//! would change the actual Bézier geometry. Both data planes are read with
//! integer `textureLoad` positions, independent of texture sampling state.
/// Creates the bind layout matching SlugParams and three texture loads.
///
/// Bindings are listed in WGSL declaration order so shader validation can
/// compare declared resource types with the actual GPU formats at pipeline
/// creation. The `None` counts mean one resource per binding, not arrays.
pub(super) fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    // WebGPU validates each binding against the WGSL declaration and the
    // actual texture view; a mismatch should fail during pipeline setup.
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Slug bind layout"),
        entries: &[
            // Uniform rows are consumed only by vertex expansion. Keeping
            // fragment visibility off avoids an unnecessary stage resource.
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // Curve points are raw f32 values. The format cannot support a
            // filtering sampler, so this type is explicitly unfilterable.
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            // Headers and curve locations are exact 16-bit integers inside
            // an unsigned texture; the shader reads them as u32 components.
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            // M11 gradients and enclosing clips use this f32 texture binding.
            // Separate evaluation preserves the original root/coverage functions.
            // Stops and affine coefficients need exact unfiltered texel loads.
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    })
}

/// Binds the current atlas planes and the fixed paint row without a sampler.
///
/// Each view remains owned by its texture store or pipeline. Rebuilding a
/// bind group is necessary after a store grows, because the old view still
/// names the old texture even when packed texel coordinates remain stable.
pub(super) fn bindings(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    curves: &wgpu::TextureView,
    bands: &wgpu::TextureView,
    paint: &wgpu::TextureView,
) -> wgpu::BindGroup {
    // There is one atlas per bound Slug batch. Growth can replace a view,
    // but the same layout and shader remain valid for every new binding.
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Slug texture and viewport bindings"),
        layout,
        entries: &[
            // The same uniform buffer is reused when DPR or target size
            // changes; queue writes replace only its eighty bytes.
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            // Curves, bands and paint occupy separate typed textures so the
            // shader cannot reinterpret an integer band as a float point.
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(curves),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(bands),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(paint),
            },
        ],
    })
}

/// Compiles the port and fixes the 56-byte vertex attribute offsets.
///
/// Vertex formats match `SlugInstance` field widths exactly: ten floats
/// occupy forty bytes, then four u32 words occupy sixteen. A mismatch here
/// can produce a plausible but wrong glyph, so the CPU/GPU oracle compares
/// the eventual pixels rather than only asserting the Rust struct size.
pub(super) fn pipeline(
    device: &wgpu::Device,
    bind_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    // Production keeps only the licensed Slug port in its source string;
    // diagnostic float outputs are excluded from the shipped wasm module.
    pipeline_source(
        device,
        bind_layout,
        format,
        shader_source().into(),
        "slug_fragment",
        Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        false,
    )
}

/// Adds a test-only entry point while retaining the production shader text.
#[cfg(feature = "oracle-debug")]
pub(super) fn debug_pipeline(
    device: &wgpu::Device,
    bind_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    // WGSL has no include directive. Joining the files only in this native
    // feature keeps debug output and its string out of shipped wasm builds.
    // The appended entry receives the same VertexOutput as the production
    // fragment, so input capture cannot silently use another vertex layout.
    let source = format!(
        "{}\n{}",
        shader_source(),
        include_str!("../shaders/slug_debug.wgsl")
    );
    pipeline_source(
        device,
        bind_layout,
        format,
        source.into(),
        "fs_debug_inputs",
        None,
        true,
    )
}

/// Pins byte offsets of the 56-byte instance independently of shader mode.
fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
    // The compile-time table is shared by normal and diagnostic pipelines;
    // a diverging layout would invalidate the math-fidelity comparison.
    const ATTRIBUTES: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
        0 => Float32x4, 1 => Float32x2, 2 => Float32x4,
        3 => Uint32, 4 => Uint32, 5 => Uint32, 6 => Uint32
    ];
    // Four shader vertices select corners of one instance; advancing the
    // buffer for every vertex would read unrelated shape bytes as a corner.
    wgpu::VertexBufferLayout {
        array_stride: 56,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &ATTRIBUTES,
    }
}

/// Chooses coverage blending or raw float diagnostic storage.
fn color_target(
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> wgpu::ColorTargetState {
    // Production colors are premultiplied; the diagnostic pass has no
    // blending so its four floats survive unchanged for CPU evaluation.
    wgpu::ColorTargetState {
        format,
        blend,
        write_mask: wgpu::ColorWrites::ALL,
    }
}

/// Compiles one Slug vertex stage with a selected diagnostic or paint output.
fn pipeline_source(
    device: &wgpu::Device,
    bind_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    source: std::borrow::Cow<'_, str>,
    fragment: &'static str,
    blend: Option<wgpu::BlendState>,
    diagnostic_index: bool,
) -> wgpu::RenderPipeline {
    // Wgpu validates the combined WGSL and binding layout during creation.
    // A missing input field or unsupported texture format fails this gate.
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Slug reference WGSL port"),
        source: wgpu::ShaderSource::Wgsl(source),
    });
    // The layout is explicit so the browser and native backends agree on
    // resource visibility and sample type without inferred differences.
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Slug pipeline layout"),
        bind_group_layouts: &[Some(bind_layout)],
        immediate_size: 0,
    });
    // Diagnostic targets preserve the contributing instance and its own
    // coverage before later overlapping glyphs blend into the color target.
    let mut targets = vec![Some(color_target(format, blend))];
    if diagnostic_index {
        targets.push(Some(color_target(wgpu::TextureFormat::R32Uint, None)));
        targets.push(Some(color_target(wgpu::TextureFormat::R32Float, None)));
    }
    // Both entry points draw the same indexed quad with one instance step;
    // only fragment outputs and blending differ for diagnostics.
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Slug instanced pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("slug_vertex"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(instance_layout())],
        },
        primitive: wgpu::PrimitiveState::default(),
        // Analytic edge coverage needs neither MSAA nor a depth attachment.
        // The target preserves one quantized coverage sample per pixel.
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some(fragment),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Composes color evaluation with the untouched Slug geometry shader.
fn shader_source() -> String {
    // WGSL declaration order does not affect calls between the two modules.
    format!(
        "{}\n{}",
        include_str!("../shaders/slug.wgsl"),
        include_str!("../shaders/paint.wgsl")
    )
}

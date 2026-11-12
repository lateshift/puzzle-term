//! Bounded raster texture arrays preserve old pages during growth (§10.3).
//!
//! One layer is allocated initially. A cold upload grows by whole pages, copies every existing layer and replaces bindings before drawing.
//! Array indices remain stable, matching the CPU mirror's shelf addresses.
//! No readback or font preparation is needed for this operation.
//! Each array has fixed page dimensions and a variable layer count. The
//! initial one-layer view still declares D2Array to match the WGSL type.
//! Default D2 views would fail validation when bound to an array declaration.
//! Both arrays share the same nearest sampler and per-frame viewport uniform.
//!
//! Coverage uses R8Unorm; authored colors use straight RGBA8Unorm samples.
//! The fragment shader performs premultiplication after sampling the color.
//! All layers remain at mip level zero, avoiding uninitialized mip sampling.
//! The caller checks the context byte allowance and device layer limits
//! before either array grows.
//!
//! Growth copies the complete old extent, including every already uploaded
//! layer. A prior terminal may draw those layers without another dirty span.
//! Queue order preserves preceding incremental writes before the copy command.
//! Following writes address the replacement texture after the copy submission.
//! New bind-group views cover all replacement layers before a draw uses them.
//! Assigning the replacement retires the old Rust owner after recording copies.
//! No CPU mirror readback or blocking GPU wait is part of array growth.
//! Paint-table growth also replaces this binding, so affine bitmap rows never
//! read the previous texture after Slug paints migrate to a larger allocation.
/// Creates a 2048-square array with an explicit coverage/color format.
pub(super) fn atlas(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    layers: u32,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("raster page array"),
        size: wgpu::Extent3d {
            width: super::PAGE,
            height: super::PAGE,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        // COPY_SRC preserves occupied layers when a new page is first needed.
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}
/// Rebinds both arrays with D2Array views even when they contain one page.
pub(crate) fn bindings(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    viewport: &wgpu::Buffer,
    mask: &wgpu::Texture,
    color: &wgpu::Texture,
    sampler: &wgpu::Sampler,
    paint: &wgpu::TextureView,
) -> wgpu::BindGroup {
    // Explicit array views cover the current capacity, including one-layer startup.
    let descriptor = wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    };
    let mask = mask.create_view(&descriptor);
    let color = color.create_view(&descriptor);
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("raster array bindings"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&mask),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&color),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            // Vertex-only effect metadata shares the existing Slug paint texture.
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(paint),
            },
        ],
    })
}
/// Grows after the caller validates its page budget; returns whether bindings changed.
pub(super) fn grow(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &mut wgpu::Texture,
    needed: u32,
) -> bool {
    // The full old extent is the copy size, not just the most recently touched page.
    let old = texture.size();
    if needed <= old.depth_or_array_layers {
        return false;
    }
    // Allocate only requested pages; rounding would overspend a non-default budget.
    let replacement = atlas(device, texture.format(), needed);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("raster array growth"),
    });
    // Queue ordering puts this copy after prior writes and before the cold upload.
    encoder.copy_texture_to_texture(texture.as_image_copy(), replacement.as_image_copy(), old);
    queue.submit([encoder.finish()]);
    *texture = replacement;
    true
}

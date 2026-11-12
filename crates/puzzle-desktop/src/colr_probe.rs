//! Native COLR gradient/clip oracle and candidate golden crops (M11.2).
//!
//! Actual brushes come from the approved font. Each layer is compared at
//! captured GPU inputs before the complete consecutive-layer image is saved.
//! The fixture is read from disk only by this development command, not bundled.
//!
//! A golden image alone would not prove gradient geometry. The independent
//! oracle first renders each fill in isolation, then captures its exact GPU
//! interpolated root em coordinate and pixel footprint with the existing M3
//! debug attachment. CPU sampling at those inputs isolates paint math from
//! rasterizer interpolation. The color oracle uses the actual CPAL stop data,
//! not a hand-authored red/blue approximation of a particular fixture glyph.
//! It multiplies the leaf coverage by each frozen enclosing Slug clip, then
//! compares every RGB component against the production premultiplied output.
//! Clear pixels are checked too, catching paint leaking outside the leaf quad.
//!
//! The matrix includes linear, radial, sweep and solid fills, several extend
//! modes, a transformed gradient, nested clips and a ten-fill graph. Every
//! graph is drawn at DPR 1 and 2. A fresh executor starts with two paint rows:
//! the multi-fill graphs necessarily grow that GPU texture and preserve the
//! earlier rows. Full-glyph crops are written only after every layer passes.
//! Pixel equality after real Device::destroy and a fresh device request proves
//! restoration uses retained CPU geometry and paints without rereading a font.
//! The user still judges the resulting reference candidates visually (R15).
//! This command reports fixture results directly because it is a native
//! development binary interface, not library logging or a shipped terminal UI.
use crate::{block_on::block_on, png};
use puzzle_render::{FrameRenderer, Renderer};
use puzzle_term::{
    color_scene::ColorScene,
    frame::{Batch, PipelineKind, RenderFrame, ZSlot},
    text::FontData,
};
/// Tests every brush/extend family and transformed, nested-clip layers.
pub(crate) fn run() -> Result<(), String> {
    // Adapter creation remains at the native platform boundary (R9).
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = block_on(Renderer::new(&instance, None)).map_err(|e| e.to_string())?;
    // Reading a repository test path avoids embedding the fixture in a product binary.
    let bytes = std::fs::read("assets/fonts/test/test_glyphs-glyf_colr_1.ttf")
        .map_err(|e| e.to_string())?;
    let data = FontData::new(bytes, 0);
    std::fs::create_dir_all("target/colr").map_err(|e| e.to_string())?;
    // Both device scales share the same size-independent paths and paint records.
    for dpr in [1, 2] {
        for glyph in [8, 9, 10, 11, 12, 13, 14, 54, 55, 56, 156, 180, 205] {
            // Each case begins with first-use geometry and dirty rows, never stale target data.
            let mut scene = ColorScene::new(data.clone(), glyph, dpr)?;
            // Creating an empty GPU store forces uploads and its subsequent growth path.
            let mut executor = FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm);
            // Upload once, then compare isolated layers using the real paint rows.
            executor
                .write_frame_uploads(&renderer, &mut scene.frame)
                .map_err(|e| e.to_string())?;
            for index in 0..scene.handles.len() {
                compare_layer(&renderer, &mut executor, &mut scene, index)?;
            }
            let (image, _) = executor
                .render_to_image(&renderer, &mut scene.frame)
                .map_err(|e| e.to_string())?;
            // The oracle must not accept two accidentally blank images as a match.
            let ink = image
                .pixels
                .chunks_exact(4)
                .filter(|p| p[..3] != [0, 0, 0])
                .count();
            if ink == 0 {
                return Err(format!("COLR {glyph} DPR {dpr} has no ink"));
            }
            // These crops are evidence candidates, not self-certified reference approval.
            png::write(
                std::path::Path::new(&format!("target/colr/{glyph}-{dpr}x.png")),
                &image,
            )?;
            if glyph == 205 && dpr == 2 {
                restore(&instance, &renderer, &mut scene, &image)?;
            }
            println!(
                "PASS COLR glyph={glyph} dpr={dpr} layers={} ink={ink}",
                scene.handles.len()
            );
        }
    }
    renderer.device().destroy();
    Ok(())
}
/// Destroys the real device and proves exact pixels after CPU-mirror restoration.
fn restore(
    instance: &wgpu::Instance,
    renderer: &Renderer,
    scene: &mut ColorScene,
    image: &puzzle_render::RgbaImage,
) -> Result<(), String> {
    // Destroy the actual device, then upload the CPU mirrors to a
    // fresh one. Equality forbids silent missing paint/clip rows.
    renderer.device().destroy();
    let restored = block_on(Renderer::new(instance, None)).map_err(|e| e.to_string())?;
    let mut fresh = FrameRenderer::new(&restored, wgpu::TextureFormat::Rgba8Unorm);
    // This operation copies encoded rows; it never invokes ColorPainter.
    scene.restore_uploads()?;
    let (after, _) = fresh
        .render_to_image(&restored, &mut scene.frame)
        .map_err(|e| e.to_string())?;
    // Exact equality covers all layer overlaps as well as individual brush checks.
    if image.pixels != after.pixels {
        return Err("COLR pixels differ after device restoration".into());
    }
    // Explicitly release the temporary GPU owner after its last readback completes.
    restored.device().destroy();
    println!("PASS COLR actual device destroy/recreate and CPU-mirror restore");
    Ok(())
}

/// Requires every pixel to agree with the CPU gradient and clip oracle ≤1/255.
fn compare_layer(
    renderer: &Renderer,
    executor: &mut FrameRenderer,
    scene: &mut ColorScene,
    index: usize,
) -> Result<(), String> {
    // A single fill avoids source-over overlap obscuring a wrong stop interpolation.
    let instance = scene.frame.slugs[index];
    // Capture and production targets have identical device extents and viewport.
    let [width, height] = scene.frame.target_size;
    // Keep its actual paint handle while drawing only one instance in the text slot.
    let mut frame = isolated_frame(instance, width, height);
    let (image, _) = executor
        .render_to_image(renderer, &mut frame)
        .map_err(|e| e.to_string())?;
    // Debug capture uses the same vertex expansion, but writes em/fwidth as floats.
    let inputs = renderer
        .render_slug_debug_inputs(&mut scene.atlas, &[instance], width, height)
        .map_err(|e| e.to_string())?;
    let paint = &scene.paints[index];
    // Comparing unquantized expected bytes permits exactly one byte of GPU error.
    for (pixel, input) in image.pixels.chunks_exact(4).zip(inputs) {
        // Zero is the debug attachment clear value, outside the expanded quad.
        if input.instance_index == 0 {
            if pixel[..3] != [0, 0, 0] {
                return Err("COLR paints outside its quad".into());
            }
            continue;
        }
        // Each outer clip was frozen in the same root-em space as this input.
        // Leaf coverage comes from the unchanged production ray functions.
        let mut coverage = input.coverage;
        for clip in &paint.clips {
            coverage *= puzzle_term::slug::reference::sample(
                &scene.atlas,
                *clip,
                [input.x, input.y],
                [input.ems_x, input.ems_y],
            )
            .map_err(|e| e.to_string())?;
        }
        // Color-line evaluation returns premultiplied sRGB, ready for coverage.
        let color = paint.sample([input.x, input.y]);
        // The target clears opaque black; RGB is therefore source RGB times coverage.
        for channel in 0..3 {
            // Compare against a float byte value before any CPU-side quantization.
            let expected = color[channel] * coverage * 255.0;
            if (f32::from(pixel[channel]) - expected).abs() > 1.001 {
                return Err(format!(
                    "COLR layer {index} em={:?} brush={:?}: channel {channel} GPU={} oracle={expected}",
                    [input.x, input.y],
                    paint.brush,
                    pixel[channel]
                ));
            }
        }
    }
    Ok(())
}

/// Isolates one production layer without changing its transform or paint address.
fn isolated_frame(
    instance: puzzle_term::frame::SlugInstance,
    width: u32,
    height: u32,
) -> RenderFrame {
    // A single text-slot draw leaves overlap blending out of the brush comparison.
    RenderFrame {
        target_size: [width, height],
        clear: [0.0, 0.0, 0.0, 1.0],
        slugs: vec![instance],
        batches: vec![Batch {
            slot: ZSlot::Text,
            pipeline: PipelineKind::Slug,
            instances: 0..1,
            scissor: None,
        }],
        ..RenderFrame::default()
    }
}

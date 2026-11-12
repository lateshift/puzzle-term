//! A mixed rectangle and Slug frame through the production contract (§10.2).
//!
//! Seven deliberately separate batches exercise z-order and scissors while
//! the GPU image gives a small golden for the new executor. The outline is
//! JetBrains Mono's capital B, already covered by M3's stricter oracle; the
//! colored rectangles make a wrong batch range or premultiplication visible.
//!
//! The approved font is loaded by the native shell, so the scene cannot
//! accidentally reintroduce a TTF into a platform-neutral or wasm binary.
//! Its `SlugScene` owns one CPU atlas mirror and the exact packed handle
//! embedded in its instance. No shape is prepared a second time for this
//! check; doing so could hide a texture-address mismatch.
//!
//! The bar instances occupy individual batch ranges rather than one range
//! because the frame contract must preserve a producer's painter order.
//! Their colors expose range-offset errors more clearly than identical
//! backgrounds would. Alternating square and rounded corners exercises
//! both rect shader branches without adding a separate pipeline.
//! The vector glyph appears in the Text slot after every Background bar.
//! All batches have one full-target scissor, making a missed clip state
//! visible while leaving the glyph's antialiased fringe inside the target.
//!
//! `draws=7` is a deliberately small contract probe for P5. Production
//! rows will merge equal-color backgrounds into larger rect batches, but
//! they must use the same `FrameRenderer::render_view` path. Nonblack ink
//! is asserted independently of the golden comparison so blessing a blank
//! PNG could never make an empty GPU pass mechanically acceptable.
use crate::{block_on::block_on, font_data, png};
use puzzle_render::{FrameRenderer, Renderer};
use puzzle_term::{
    frame::{Batch, PipelineKind, RectInstance, RectU32, RenderFrame, Upload, ZSlot},
    slug::atlas::TEXTURE_WIDTH,
    slug_scene::{SlugScene, SlugSceneKind},
};
use std::path::Path;

/// Renders and mechanically checks the fixed contract scene.
pub(crate) fn run() -> Result<(), String> {
    // Only this platform shell creates a GPU instance or reads font bytes;
    // neither action leaks into the platform-neutral frame builder (R9).
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = block_on(Renderer::new(&instance, None)).map_err(|error| error.to_string())?;
    let font = font_data::bundled_font()?;
    // This 2× B includes a curved counter; a missing Slug upload would
    // leave the large central specimen black while the bars still draw.
    let mut scene = SlugScene::new(SlugSceneKind::GlyphLarge, Some(&font))?;
    let mut frame = build_frame(&mut scene);
    let mut executor = FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm);
    let (image, draws) = executor
        .render_to_image(&renderer, &mut frame)
        .map_err(|error| error.to_string())?;
    if draws != 7 {
        return Err(format!("expected 7 mixed frame draws, got {draws}"));
    }
    let ink = image
        .pixels
        .chunks_exact(4)
        .filter(|pixel| pixel[..3] != [0, 0, 0])
        .count();
    if ink < 1000 {
        return Err(format!(
            "mixed frame is blank or truncated: {ink} ink pixels"
        ));
    }
    let path = Path::new("target/frame/001-rect-slug.png");
    // The path is a new candidate reference. Human review under R15 is
    // separate from this probe's byte comparison and ink threshold.
    std::fs::create_dir_all("target/frame").map_err(|error| error.to_string())?;
    png::write(path, &image)?;
    println!("draws={draws} ink={ink} path={}", path.display());
    Ok(())
}

/// Builds ordered rectangle and glyph batches independently of wgpu.
fn build_frame(scene: &mut SlugScene) -> RenderFrame {
    let mut frame = RenderFrame {
        target_size: [scene.width, scene.height],
        clear: [0.0, 0.0, 0.0, 1.0],
        ..RenderFrame::default()
    };
    // The Slug CPU mirror marks only texels prepared for this B. Uploading
    // those row-local spans tests the incremental frame contract as well.
    append_uploads(&mut frame, scene);
    // Instance contents are plain 56-byte data. The executor never reads
    // a glyph outline or a font during its frame submission.
    frame.slugs.extend_from_slice(&scene.instances);
    // Six colored bars occupy one painter slot. A full-target clip
    // tests scissor setup without cutting antialiased B fringe pixels.
    for (index, color) in [
        0xff_3a_7b_d5,
        0xff_73_c9_50,
        0xff_e1_81_42,
        0xff_a2_76_d2,
        0xff_42_be_c8,
        0xff_e0_d0_60,
    ]
    .into_iter()
    .enumerate()
    {
        // Each horizontal bar is wholly inside the 248 px scene crop;
        // choosing fixed device pixels avoids DPR-dependent CSS rounding.
        frame.rects.push(RectInstance {
            rect: [8.0 + index as f32 * 36.0, 226.0, 28.0, 8.0],
            color,
            corner_radius: if index % 2 == 0 { 3.0 } else { 0.0 },
            stroke_width: 0.0,
            flags: 0,
        });
    }
    let clip = Some(RectU32 {
        x: 0,
        y: 0,
        width: scene.width,
        height: scene.height,
    });
    for index in 0..6 {
        // A unique one-element range pins vertex-buffer offsets. If
        // ranges are ignored, all six draws repeat the first bar's color.
        frame.batches.push(Batch {
            slot: ZSlot::Background,
            pipeline: PipelineKind::Rect,
            instances: index..index + 1,
            scissor: clip,
        });
    }
    frame.batches.push(Batch {
        slot: ZSlot::Text,
        pipeline: PipelineKind::Slug,
        instances: 0..frame.slugs.len() as u32,
        scissor: clip,
    });
    frame
}

/// Converts the Slug atlas's dirty mirror ranges into neutral Upload values.
fn append_uploads(frame: &mut RenderFrame, scene: &mut SlugScene) {
    let (curves, bands) = scene.atlas.take_dirty();
    // Dirty spans name physical 4096-texel rows. Retaining their exact
    // address protects packed glyph handles from a second placement pass.
    for span in curves {
        // Dirty offsets refer to the physical mirror, including padding
        // skipped when a curve list would cross a texture row (rule B3).
        let first = span.row as usize * TEXTURE_WIDTH + span.start as usize;
        let last = span.row as usize * TEXTURE_WIDTH + span.end as usize;
        frame.uploads.push(Upload::Curves {
            origin: [u32::from(span.start), u32::from(span.row)],
            texels: scene.atlas.curve_texels()[first..last].to_vec(),
        });
    }
    for span in bands {
        // Band headers are written after lists and may extend a dirty
        // span backward; slicing the final mirror preserves that order.
        let first = span.row as usize * TEXTURE_WIDTH + span.start as usize;
        let last = span.row as usize * TEXTURE_WIDTH + span.end as usize;
        frame.uploads.push(Upload::Bands {
            origin: [u32::from(span.start), u32::from(span.row)],
            texels: scene.atlas.band_texels()[first..last].to_vec(),
        });
    }
}

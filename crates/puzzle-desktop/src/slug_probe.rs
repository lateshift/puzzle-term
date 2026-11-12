//! Native two-layer M3 oracle verification (design §9.6).
//!
//! The production fragment computes coverage from interpolated em
//! coordinates. A second, test-only fragment records those exact inputs in
//! Rgba32Float. Comparing the CPU oracle at those inputs proves coverage
//! math fidelity within one byte. The end-to-end probe checks the captured
//! coordinate displacement and explains image differences with the oracle
//! at both captured and exact-affine coordinates.
//!
//! All 33 geometry and real-font scenes run in both layers. Successful
//! end-to-end renders write PNG candidates for the reviewer's visual check.
//! The platform shell creates the GPU instance, preserving R9 boundaries.
//!
//! The two comparisons answer different questions. Math mode asks whether
//! the WGSL root solver, band walks and coverage combiner produce the same
//! byte as the licensed Rust port when both see identical inputs. It fails
//! for a single channel more than one byte apart. End-to-end mode asks
//! whether vertex expansion and rasterization stay within 1/256 device
//! pixel and whether every image difference is attributable to that measured
//! coordinate displacement, with one final byte for quantization.
//!
//! Ink is checked independently of either comparison. A blank target could
//! otherwise agree with an accidental blank oracle or a skipped debug pass.
//! Scene PNGs are written only by end-to-end mode, after that mechanical
//! gate succeeds. Human visual judgment of those images remains pending.
//!
//! Multi-glyph specimen lines use two more diagnostic attachments: an
//! instance index chooses the matching atlas handle, and per-fragment
//! coverage avoids comparing a single glyph with a blended overlap.
//! The probe reports every scene in a failing matrix instead of stopping
//! at the first size. That matters when deciding whether interpolation
//! errors are isolated to one pixel configuration or widespread.
use crate::block_on::block_on;
use puzzle_render::{Renderer, RgbaImage, SlugDebugPixel};
use puzzle_term::slug_scene::{FontData, SlugScene, SlugSceneKind};
/// Per-instance image comparison using captured fragment inputs.
mod end_to_end;
#[cfg(test)]
/// D-39 relocation must draw correct pixels through the retained production executor.
#[path = "slug_probe/pressure/tests.rs"]
mod pressure_tests;

/// Chooses math fidelity or the image-level end-to-end comparison.
pub(crate) fn run(mode: Option<&str>) -> Result<(), String> {
    // A named mode lets `accept M3` report math and image criteria on
    // separate PASS/FAIL lines without one failure hiding the other.
    let mode = mode.unwrap_or("end-to-end");
    if mode != "math" {
        return end_to_end::run(mode);
    }
    // Only the platform binary selects an adapter. The renderer receives
    // its device and can then use the same atlas on native and web targets.
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = block_on(Renderer::new(&instance, None)).map_err(|error| error.to_string())?;
    let font = crate::font_data::bundled_font()?;
    // Collect only fixture-level errors. Device discovery still aborts
    // immediately because no later fixture could succeed without a device.
    let mut failures = Vec::new();
    for kind in SlugSceneKind::all() {
        // Keep testing after one failure so a large matrix reveals every
        // size or transform that needs investigation in one GPU session.
        match run_scene(&renderer, &font, kind) {
            Ok(ink) => println!("PASS math {} ink={ink}", kind.name()),
            Err(error) => failures.push(format!("{}: {error}", kind.name())),
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        // Newlines preserve each fixture's measured count and first pixel
        // in the acceptance report's failure reason.
        Err(failures.join("\n"))
    }
}

/// Draws and verifies one fixture with a fresh first-use atlas upload.
fn run_scene(renderer: &Renderer, font: &FontData, kind: SlugSceneKind) -> Result<usize, String> {
    let mut scene = SlugScene::new(kind, Some(font))?;
    // GPU uploads use the packed mirror from this scene. The oracle later
    // reads the same mirror, excluding a second packing path as a cause.
    let image = renderer
        .render_slug_to_image(
            &mut scene.atlas,
            &scene.instances,
            scene.width,
            scene.height,
        )
        .map_err(|error| error.to_string())?;
    // A missing upload or a constant-white shader must fail even if an
    // accidental blank oracle would otherwise agree with the target.
    let ink = image
        .pixels
        .chunks_exact(4)
        .filter(|pixel| pixel[0] != 0)
        .count();
    if ink == 0 || ink == (image.width * image.height) as usize {
        return Err(format!("image has {ink} ink pixels"));
    }
    // This path deliberately checks a single diagnostic pass; end-to-end
    // mode captures each instance separately to explain overlap pixels.
    verify_math(renderer, &mut scene, &image)?;
    Ok(ink)
}

/// Checks every shaded pixel against the CPU oracle at GPU inputs.
fn verify_math(
    renderer: &Renderer,
    scene: &mut SlugScene,
    image: &RgbaImage,
) -> Result<(), String> {
    // This pass uses the same vertex stage and instance as the coverage
    // pass, but its float target preserves coordinates before quantization.
    let debug = renderer
        .render_slug_debug_inputs(
            &mut scene.atlas,
            &scene.instances,
            scene.width,
            scene.height,
        )
        .map_err(|error| error.to_string())?;
    let mut shaded = 0;
    // Pixel order is row-major in both readbacks. The same index therefore
    // names the production color and its diagnostic inputs.
    for (index, (inputs, pixel)) in debug.iter().zip(image.pixels.chunks_exact(4)).enumerate() {
        // A zero derivative is the transparent clear outside the dilated
        // quad, not an input to the reference's reciprocal derivative.
        if inputs.instance_index == 0 {
            continue;
        }
        shaded += 1;
        // The CPU oracle receives the actual GPU derivative, avoiding an
        // analytic-fwidth assumption near quad or triangle boundaries.
        let expected = reference_at(scene, inputs)?;
        let actual = (inputs.coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
        if actual.abs_diff(expected) > 1 {
            return Err(format!(
                "math pixel {index} ({},{}), glyph={} at=({},{}) GPU={actual} CPU={expected}",
                index as u32 % scene.width,
                index as u32 / scene.width,
                inputs.instance_index,
                inputs.x,
                inputs.y
            ));
        }
        // A single-quad fixture has no overlapping instances, so its
        // production unorm color must also agree with the captured coverage.
        if scene.instances.len() == 1
            && pixel[..3]
                .iter()
                .any(|channel| channel.abs_diff(expected) > 1)
        {
            return Err(format!(
                "math pixel {index}: production color differs from its coverage"
            ));
        }
    }
    if shaded == 0 {
        return Err("debug fragment captured no Slug pixels".into());
    }
    Ok(())
}

/// Converts a diagnostic RGBA32Float pixel into an oracle coverage byte.
fn reference_at(scene: &SlugScene, inputs: &SlugDebugPixel) -> Result<u8, String> {
    scene.oracle_at(
        (inputs.instance_index - 1) as usize,
        [inputs.x, inputs.y],
        [inputs.ems_x, inputs.ems_y],
    )
}

/// Times D-8 curve formats with identical real JetBrains Mono glyph batches.
pub(crate) fn benchmark() -> Result<(), String> {
    // The platform chooses one adapter for both variants, so a reported
    // ratio cannot mix integrated and discrete GPU measurements.
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = block_on(Renderer::new(&instance, None)).map_err(|error| error.to_string())?;
    let font = crate::font_data::bundled_font()?;
    let mut scene = SlugScene::new(SlugSceneKind::Glyph, Some(&font))?;
    let instance = *scene
        .instances
        .first()
        .ok_or("glyph scene has no instance")?;
    let (full, half) = renderer
        .benchmark_slug_formats(&mut scene.atlas, instance)
        .map_err(|error| error.to_string())?;
    let overhead = (full / half - 1.0) * 100.0;
    println!(
        "D-8 B glyph 16384 instances: f32={full:.4} ms f16={half:.4} ms overhead={overhead:.2}%"
    );
    Ok(())
}

/// Checks that old and newly uploaded curve/band texels survive GPU growth.
pub(crate) fn store_growth() -> Result<(), String> {
    // GPU discovery remains a platform-shell concern. The renderer's probe
    // performs exact texel readback after both 64-row stores double.
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = block_on(Renderer::new(&instance, None)).map_err(|error| error.to_string())?;
    let (curves, bands) = renderer
        .probe_slug_store_growth()
        .map_err(|error| error.to_string())?;
    println!("Slug GPU store growth: curve_rows={curves} band_rows={bands} old/new texels match");
    Ok(())
}

/// Proves paint index one samples the table while index zero remains solid.
pub(crate) fn paint_table() -> Result<(), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = block_on(Renderer::new(&instance, None)).map_err(|error| error.to_string())?;
    let font = crate::font_data::bundled_font()?;
    let mut scene = SlugScene::new(SlugSceneKind::Glyph, Some(&font))?;
    // Green comes only from the paint texture: the instance's packed color
    // remains white, so a skipped table fetch produces a visible failure.
    let instance = scene
        .instances
        .first_mut()
        .ok_or("glyph scene has no instance")?;
    instance.paint = 1;
    let image = renderer
        .render_slug_paint_to_image(
            &mut scene.atlas,
            &scene.instances,
            scene.width,
            scene.height,
            [0.0, 1.0, 0.0, 1.0],
        )
        .map_err(|error| error.to_string())?;
    let ink = image
        .pixels
        .chunks_exact(4)
        .filter(|pixel| pixel[1] > 0 && pixel[0] == 0 && pixel[2] == 0)
        .count();
    if ink == 0 {
        return Err("nonzero paint index did not produce green glyph ink".into());
    }
    println!("Slug paint-table index 1: green_ink={ink}");
    Ok(())
}

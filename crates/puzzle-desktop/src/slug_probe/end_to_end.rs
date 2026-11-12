//! End-to-end Slug diagnostics with measured interpolation (§9.6).
//!
//! The exact-affine CPU image is retained as the baseline. For every draw
//! instance, a diagnostic pass captures the rasterizer's em coordinate and
//! derivative without changing the production shader. Evaluating the CPU
//! oracle at those inputs gives a second image in the same draw order.
//! Comparing the three images isolates interpolation from coverage math.
//!
//! A single capture of a text line preserves only its last overlapping
//! fragment at a pixel. Capturing each instance alone is slower but keeps
//! every contributing coordinate, including the earlier glyph responsible
//! for the 10 px punctuation case. This runs only in the native M3 gate.
//!
//! The coordinate check uses the full affine matrix to convert em-space
//! displacement into physical pixels. It therefore applies to rotations
//! and shears, not merely to horizontal text. The maximum over the entire
//! 33-scene suite is reported when the fixed 1/256 px bound is exceeded.
//!
//! The deviation check uses final quantized grayscale bytes: GPU versus
//! exact-affine oracle may differ only as much as the same oracle changes
//! when given the captured GPU coordinates, plus one coverage byte. The
//! reference shader's equations and the golden image tolerance are intact.
//!
//! Three grayscale byte arrays have different roles. `exact` samples the
//! CPU oracle with the mathematical inverse affine position at each pixel
//! centre. `at_gpu` samples the same oracle with the GPU's captured position
//! and derivative for each contributing instance. `image` is the actual
//! production render target. The reported excess is the amount by which
//! `image` is farther from `exact` than `at_gpu` is, after one byte of
//! quantization allowance. No error percentage can hide a local defect.
//!
//! Both oracle images composite in the order of the submitted instances.
//! The target is opaque black and every glyph is opaque white, so each
//! color channel carries the same coverage byte. Alpha stays opaque; the
//! comparison uses red only because green and blue contain the same value
//! in these deliberately monochrome fixtures.
use crate::{block_on::block_on, font_data, png};
use puzzle_render::{Renderer, RgbaImage};
use puzzle_term::slug_scene::{SlugScene, SlugSceneKind};
use std::{fs, path::Path};

/// The greatest coordinate or explained-deviation error seen in the suite.
#[derive(Default)]
struct Observation {
    /// Error in device pixels for coordinates or bytes for deviation.
    value: f32,
    /// Named fixture containing this maximum.
    scene: String,
    /// Exact location in device pixels.
    pixel: [u32; 2],
    /// Draw-instance index for a coordinate witness; final pixels mix glyphs.
    glyph: Option<usize>,
    /// Captured values that make the measurement reproducible.
    detail: String,
}

impl Observation {
    /// Keeps the largest error rather than the first failure.
    fn include(
        &mut self,
        value: f32,
        scene: &str,
        pixel: [u32; 2],
        glyph: Option<usize>,
        detail: String,
    ) {
        // A suite-wide maximum is more useful than the first failure when
        // the same rounding problem appears at multiple sizes and DPRs.
        if value > self.value {
            self.value = value;
            self.scene = scene.into();
            self.pixel = pixel;
            self.glyph = glyph;
            self.detail = detail;
        }
    }

    /// Formats the measured witness for an acceptance failure.
    fn evidence(&self) -> String {
        // The glyph index distinguishes two overlapping outlines at one
        // pixel; the scene name maps directly to a reference PNG filename.
        format!(
            "max={:.8} scene={} pixel=({}, {}) glyph={:?} {}",
            self.value, self.scene, self.pixel[0], self.pixel[1], self.glyph, self.detail
        )
    }
}

/// Measurements gathered from all 33 scenes before deciding either gate.
#[derive(Default)]
struct Suite {
    /// Greatest interpolated-coordinate displacement in physical pixels.
    coordinate: Observation,
    /// Greatest positive byte excess beyond oracle sensitivity plus one.
    deviation: Observation,
}

/// Runs one or both revised §9.6 checks without changing their bounds.
pub(super) fn run(mode: &str) -> Result<(), String> {
    if !["coordinate", "explained", "end-to-end"].contains(&mode) {
        return Err(
            "usage: puzzle-desktop slug-probe [math|coordinate|explained|end-to-end]".into(),
        );
    }
    // Adapter selection stays in the platform binary. All instances in a
    // scene share this device and the exact same packed CPU atlas.
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = block_on(Renderer::new(&instance, None)).map_err(|error| error.to_string())?;
    let font = font_data::bundled_font()?;
    // Native fixtures use bytes from assets/fonts, never a font embedded in
    // a neutral crate or one discovered from the host operating system.
    let directory = Path::new("target/slug/gpu-scenes");
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut suite = Suite::default();
    // Running every fixture before reporting the maxima prevents a small
    // early scene from concealing a worse coordinate error at 4× zoom.
    for kind in SlugSceneKind::all() {
        let mut scene = SlugScene::new(kind, Some(&font))?;
        let image = renderer
            .render_slug_to_image(
                &mut scene.atlas,
                &scene.instances,
                scene.width,
                scene.height,
            )
            .map_err(|error| error.to_string())?;
        // A blank or full target cannot satisfy a mathematical comparison
        // by accident; the harness also checks bounded ink per fixture.
        let ink = image.pixels.chunks_exact(4).filter(|px| px[0] != 0).count();
        if ink == 0 || ink == (scene.width * scene.height) as usize {
            return Err(format!("{}: image has {ink} ink pixels", kind.name()));
        }
        measure_scene(&renderer, &mut scene, &image, &kind.name(), &mut suite)?;
        // Candidate PNGs stay in target until the reviewed golden command
        // intentionally copies them into assets/scenarios/slug.
        if mode == "end-to-end" {
            png::write(&directory.join(format!("{}.png", kind.name())), &image)?;
        }
        println!("MEASURE {mode} {} ink={ink}", kind.name());
    }
    report(mode, &suite)
}

/// Captures each contributing fragment before later glyphs can cover it.
fn measure_scene(
    renderer: &Renderer,
    scene: &mut SlugScene,
    image: &RgbaImage,
    name: &str,
    suite: &mut Suite,
) -> Result<(), String> {
    let exact = scene.oracle_pixels()?;
    // This compact grayscale plane is filled one glyph at a time. Keeping
    // only final bytes avoids retaining 30 full Rgba32Float captures for a
    // large specimen line, which would multiply readback memory by glyphs.
    let mut at_gpu = vec![0_u8; scene.width as usize * scene.height as usize];
    for glyph in 0..scene.instances.len() {
        measure_glyph(renderer, scene, glyph, name, &mut at_gpu, suite)?;
    }
    compare_images(scene, image, &exact, &at_gpu, name, suite);
    Ok(())
}

/// Captures one glyph's interpolated inputs even beneath later glyphs.
fn measure_glyph(
    renderer: &Renderer,
    scene: &mut SlugScene,
    glyph: usize,
    name: &str,
    at_gpu: &mut [u8],
    suite: &mut Suite,
) -> Result<(), String> {
    // A one-instance pass preserves the rasterizer interpolation of that
    // quad while exposing fragments hidden by later draws.
    let captured = renderer
        .render_slug_debug_inputs(
            &mut scene.atlas,
            &scene.instances[glyph..glyph + 1],
            scene.width,
            scene.height,
        )
        .map_err(|error| error.to_string())?;
    for (index, input) in captured.iter().enumerate() {
        // Zero is the diagnostic clear sentinel, so no derivative or curve
        // lookup is attempted where the quad did not rasterize.
        if input.instance_index == 0 {
            continue;
        }
        let pixel = [index as u32 % scene.width, index as u32 / scene.width];
        let affine = scene.exact_at(glyph, pixel[0], pixel[1])?;
        let gpu = [input.x, input.y];
        let error = scene.coordinate_error_px(glyph, affine, gpu)?;
        if !error.is_finite() {
            return Err(format!(
                "{name}: nonfinite coordinate at {pixel:?}, glyph {glyph}"
            ));
        }
        if error > suite.coordinate.value {
            // Formatting only a new maximum avoids allocating one string
            // per shaded pixel of the large specimen lines.
            suite.coordinate.include(
                error,
                name,
                pixel,
                Some(glyph),
                format!("exact={affine:?} captured={gpu:?}"),
            );
        }
        // Reference coverage uses the captured em coordinate and derivative.
        // Quantization follows each source-over blend in draw order.
        let coverage = scene.oracle_coverage(glyph, gpu, [input.ems_x, input.ems_y])?;
        if !coverage.is_finite() {
            return Err(format!(
                "{name}: nonfinite coverage at {pixel:?}, glyph {glyph}"
            ));
        }
        let old = f32::from(at_gpu[index]) / 255.0;
        at_gpu[index] = ((coverage + old * (1.0 - coverage)) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8;
    }
    Ok(())
}

/// Measures unexplained final-byte differences after oracle compositing.
fn compare_images(
    scene: &SlugScene,
    image: &RgbaImage,
    exact: &[u8],
    at_gpu: &[u8],
    name: &str,
    suite: &mut Suite,
) {
    for (index, (actual, reference)) in image
        .pixels
        .chunks_exact(4)
        .zip(exact.chunks_exact(4))
        .enumerate()
    {
        for channel in 0..4 {
            // RGB all carry white coverage; alpha is opaque in both CPU
            // images. Checking each channel also catches a bad color write.
            let gpu_oracle = if channel == 3 { 255 } else { at_gpu[index] };
            // The oracle's own change is the interpolation allowance; the
            // final byte covers unorm storage and CPU rounding.
            let sensitivity = gpu_oracle.abs_diff(reference[channel]);
            let difference = actual[channel].abs_diff(reference[channel]);
            let excess = i16::from(difference) - i16::from(sensitivity) - 1;
            if f32::from(excess) > suite.deviation.value {
                let pixel = [index as u32 % scene.width, index as u32 / scene.width];
                suite.deviation.include(
                    f32::from(excess),
                    name,
                    pixel,
                    None,
                    format!(
                        "channel={channel} GPU={} oracle_exact={} oracle_gpu={gpu_oracle}",
                        actual[channel], reference[channel]
                    ),
                );
            }
        }
    }
}

/// Returns one failure for each selected criterion, after all scenes ran.
fn report(mode: &str, suite: &Suite) -> Result<(), String> {
    // The bound is a physical-device-pixel displacement, independent of
    // em size, rotation and DPR. A larger value needs reviewer action.
    let coordinate_failed = suite.coordinate.value > 1.0 / 256.0;
    let deviation_failed = suite.deviation.value > 0.0;
    if (mode == "coordinate" && coordinate_failed)
        || (mode == "explained" && deviation_failed)
        || (mode == "end-to-end" && (coordinate_failed || deviation_failed))
    {
        return Err(format!(
            "coordinate: {}; explained deviation: {}",
            suite.coordinate.evidence(),
            suite.deviation.evidence()
        ));
    }
    println!("coordinate max={:.8} px", suite.coordinate.value);
    println!(
        "explained deviation max excess={:.0} byte",
        suite.deviation.value
    );
    Ok(())
}

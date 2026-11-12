//! Machine-specific M9 CJK and bitmap emoji evidence, excluded from goldens.
//!
//! The user approved installed Heiti and Apple Color Emoji only for this review.
//! A public model/view registration draws rotated and zoomed annotations.
//! Its static model supplies dirty source notifications, while the view owns
//! only rebuildable painter commands and borrows renderer-owned fonts.
//! This probe does not provide an alternate interaction or scheduling API.
//! Coverage checks reject missing glyphs before any image can be offered for review.
//! Native bitmap preparation receives the effective ppem, including transform zoom.
//! Independent strike checks show the source grows beyond the ordinary grid strike.
//! Images are labeled in the scene and written only to ignored target/m9-review.
//! No installed font is copied into assets or compared as a golden reference.
//! Coverage and strike validation intentionally precede provider drawing.
//! This separates missing installation from a renderer upload or transform defect.
//! The independent source-size witness reads selected strike metadata, not pixel count.
//! Image-region checks then prove the selected ink actually reaches the GPU.
//! System font revisions may alter the exact samples without changing these contracts.
//! Review files therefore carry no accepted-pixel threshold or stored reference.
//! Both ratios keep identical logical regions while changing source device size.
//! A failure preserves its log and does not alter the approved font policy.
//! Successful mechanical readbacks never certify visual edge quality.
use crate::{block_on::block_on, system_fonts::NativeFonts};
use puzzle_render::{FrameRenderer, Renderer};
use puzzle_term::text::geometry::Affine;
use puzzle_term::{
    Session, SessionConfig,
    frame_builder::context::TextContext,
    overlay::{
        anchor::Anchor,
        draw::{DrawList, Primitive, TextSpec},
        geometry::Rect,
        model::ViewState,
        provider::EventKinds,
        view::{OverlayView, ViewCtx},
    },
    platform::{PtySize, Services, Transport, Waker},
    text::{
        FontData, bitmap, cluster_source::ClusterRequest, face::FontFace, font_set::FontStyle,
        presentation::Presentation,
    },
};
use std::time::Instant;

/// The probe has no application; frame requests are fulfilled by explicit samples.
struct Host;
/// Replies and resize effects have no external destination in this review scene.
impl Transport for Host {
    /// Discards protocol replies while leaving the ordinary Session parser active.
    fn write(&mut self, _bytes: &[u8]) -> Result<(), String> {
        Ok(())
    }
    /// No PTY needs a resize for the fixed review viewport.
    fn resize(&mut self, _size: PtySize) -> Result<(), String> {
        Ok(())
    }
    /// There is no child process to retain after rendering.
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}
/// Both scales use explicit timestamps rather than a native event-loop timer.
impl Waker for Host {
    /// The next explicit frame fulfills the pending request.
    fn request_frame(&mut self) {}
    /// This static scene has no animation needing a future wake.
    fn wake_at(&mut self, _at: Instant) {}
}
/// Public annotation requests are independent of native discovery and image I/O.
mod scene;
/// Renders both device scales without entering the harness/golden command catalogue.
/// Either DPR failing exits nonzero; no output from this probe can bless a reference.
/// Its isolated device uses the production premultiplied raster and Slug pipelines.
pub(crate) fn run() -> Result<(), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = block_on(Renderer::new(&instance, None)).map_err(|e| e.to_string())?;
    for dpr in [1.0, 2.0] {
        draw(&renderer, dpr)?;
    }
    // Every readback is complete before destroying this isolated development device.
    renderer.device().destroy();
    Ok(())
}
/// Creates the ordinary host-owned context, then installs explicitly approved families.
/// Session owns VT/providers; the context owns shared font and glyph stores.
/// Callbacks compile after neutral row layout through the ordinary frame path.
/// Independent CJK/emoji ink assertions precede any review-image write.
/// Output paths remain ignored development data, outside pixel comparisons.
fn draw(renderer: &Renderer, dpr: f32) -> Result<(), String> {
    let mut context = TextContext::new(crate::font_data::bundled_font_set()?, 14.0, dpr)?;
    coverage(&context)?;
    // Inspect actual installed strike ppem independently of the GPU destination quad.
    strike(dpr)?;
    crate::system_fonts::install(&mut context, &["Heiti SC".into()])?;
    // Blank VT content keeps terminal output from obscuring annotation regions.
    let mut session = Session::new(
        SessionConfig {
            columns: 80,
            rows: 22,
            ..Default::default()
        },
        Services::new(Box::new(Host), Box::new(Host)),
    );
    session.set_text_context(&context);
    // Cursor paper would count as ink, so the scene explicitly suppresses it.
    session.feed(b"\x1b[?25l", Instant::now());
    session.overlays_mut().add_view(
        "M9-machine-specific-review",
        EventKinds::ALL,
        Box::new(scene::Scene),
    );
    let mut drawing = crate::system_fonts::settle::session(&mut session, &mut context)?;
    // Settling consumes the last diagnostic frame. Request its retained ink through
    // the same completion wake as the host before taking the readback frame.
    session.submit(
        puzzle_term::messages::Command::GlyphRasterized,
        puzzle_term::Origin::Platform,
    );
    session.pump(Instant::now());
    // The frame builder compiles public draws; the probe fabricates no instances.
    let frame = drawing
        .draw(&mut session, Instant::now(), &mut context)?
        .ok_or("review frame absent")?;
    if frame.rasters.len() < 2 || frame.slugs.is_empty() {
        return Err("review fallback ink absent".into());
    }
    // Pending atlas and paint uploads are applied before the blocking readback.
    let mut gpu = FrameRenderer::new(renderer, wgpu::TextureFormat::Rgba8Unorm);
    let (image, _) = gpu
        .render_to_image(renderer, frame)
        .map_err(|e| e.to_string())?;
    guard(&image, dpr)?;
    // Both the scene caption and this directory identify machine-specific evidence.
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/m9-review");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    crate::png::write(
        &out.join(format!("machine-specific-{}x.png", dpr as u32)),
        &image,
    )?;
    Ok(())
}
/// Real native coverage must name Heiti for CJK and Apple Color Emoji for the crab.
/// The complete-cluster source validator excludes macOS LastResort boxes.
/// Family names record this machine’s chosen sources, not golden requirements.
/// Missing installation fails this opted-in probe instead of silently accepting tofu.
fn coverage(context: &TextContext) -> Result<(), String> {
    let mut source = NativeFonts::new(vec!["Heiti SC".into()], false, true, context.metrics());
    // Explicit CJK preference precedes generic/script fallback and emoji selection.
    for (cluster, presentation, family) in [
        ("漢字", Presentation::Text, "Heiti SC"),
        ("🦀", Presentation::Emoji, "Apple Color Emoji"),
    ] {
        // Plain-data style/presentation values match the production layout seam.
        let request = ClusterRequest {
            cluster,
            presentation,
            style: FontStyle::Regular,
        };
        // Nonblank ink alone cannot establish coverage: a missing box is also visible.
        if source.resolve(request, &source.probe_placement()).is_none()
            || source.last_family.as_deref() != Some(family)
        {
            return Err(format!(
                "review cluster {cluster} did not resolve to {family}: {:?}",
                source.last_family
            ));
        }
        println!("M9 machine-specific coverage cluster={cluster} family={family}");
    }
    Ok(())
}
/// Checks the actual installed sbix strike selected at ordinary and zoomed device ppem.
/// Effective device size chooses a source before fitting into a neutral bitmap quad.
/// Both requests share bytes and glyph ID, isolating strike-size selection.
/// Samples are dropped after reporting; no installed font becomes an asset.
fn strike(dpr: f32) -> Result<(), String> {
    // Installed bytes are development input only; no asset or packaged font is created.
    let data = FontData::new(
        std::fs::read("/System/Library/Fonts/Apple Color Emoji.ttc").map_err(|e| e.to_string())?,
        0,
    );
    let face = FontFace::new(data.clone(), 400)?;
    // A Unicode scalar is not a font glyph ID: query the installed face’s cmap.
    let glyph = face.glyph_id('🦀')?.ok_or("native crab missing")?;
    let ordinary = bitmap::load(&data, glyph, 24.0 * dpr, &mut crate::bitmap::Decoder)?
        .ok_or("ordinary strike absent")?;
    // Threefold magnification changes the requested source, not just the final quad.
    let zoomed = bitmap::load(&data, glyph, 72.0 * dpr, &mut crate::bitmap::Decoder)?
        .ok_or("zoomed strike absent")?;
    // This installed face has suitable strikes; a small enlarged source must fail.
    if zoomed.ppem < 72.0 * dpr || zoomed.ppem < ordinary.ppem {
        return Err("zoom selected an undersized installed emoji strike".into());
    }
    println!(
        "M9 native sbix dpr={dpr} ordinary_strike={} effective_ppem={} zoomed_strike={}",
        ordinary.ppem,
        72.0 * dpr,
        zoomed.ppem
    );
    Ok(())
}
/// Separate CJK/emoji regions must contain visible submitted ink at each scale.
/// Authored logical regions scale independently of provider measurements.
/// The threshold ignores antialiasing noise but catches empty glyphs/missing uploads.
/// Mechanical ink passes leave edge quality and native appearance to human review.
fn guard(image: &puzzle_render::RgbaImage, dpr: f32) -> Result<(), String> {
    // Clear padding supplies the baseline without consulting renderer color policy.
    let background = &image.pixels[..3];
    for (x0, y0, x1, y1) in [
        (30, 75, 130, 160),
        (30, 240, 240, 380),
        (300, 75, 430, 160),
        (300, 240, 490, 380),
    ] {
        // Each fallback is checked separately so labels cannot hide an empty sample.
        let mut ink = 0;
        for y in y0 * dpr as u32..y1 * dpr as u32 {
            for x in x0 * dpr as u32..x1 * dpr as u32 {
                let at = ((y * image.width + x) * 4) as usize;
                ink += usize::from(
                    image.pixels[at..at + 3]
                        .iter()
                        .zip(background)
                        .any(|(a, b)| a.abs_diff(*b) > 8),
                );
            }
        }
        if ink < 30 {
            return Err(format!(
                "machine-specific review region {x0},{y0} has no ink"
            ));
        }
        println!("M9 machine-specific dpr={dpr} region={x0},{y0} ink={ink}");
    }
    Ok(())
}

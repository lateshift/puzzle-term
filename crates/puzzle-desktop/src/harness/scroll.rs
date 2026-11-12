//! Fractional edge-row readback through the production Session (§7.3).
//!
//! A three-row viewport over six colored text rows exposes both clipped
//! ends. Every grid layer must share the same integral offset and scissor;
//! GPU pixels in the padding must remain clear. The scenario is an authored
//! fixture, with no external recording, fonts or new dependency.
//!
//! The scene starts at the tail and moves half a device cell into history.
//! Its top row is therefore the preceding blue row, partly above the grid;
//! the last blue live row appears partly below the bottom. Omitting either
//! extra row would leave an empty strip despite correct whole-row scrolling.
//! Bright text on three distinct backgrounds lets readback distinguish
//! actual glyph ink from cell paper. Ink checks sample both clipped strips.
//!
//! The entire outer area is checked, rather than just one padding pixel:
//! glyph dilation can cross the left or right grid edge, and row backgrounds
//! can cross the top or bottom. Every batch must clip those independent
//! pipelines to the same fixed rectangle. Capturing four rows must still
//! render into a three-row attachment; an enlarged surface would conceal
//! the bottom clipping defect this fixture is meant to catch.
//!
//! Rounding happens in the viewport, before assembly. A 0.2-pixel remainder
//! deliberately tests that policy, while the model still retains its exact
//! physical position for subsequent input. Glyph origins must be integral.
//! Each DPR owns a fresh text context and GPU atlas, preventing warmed
//! hinted outlines from accidentally standing in for the unhinted path.
//! Within a DPR, two frames share resources: missing uploads or stale row
//! ownership must remain observable on the second, fractionally moved frame.
use super::Scenario;
use puzzle_render::{FrameRenderer, RgbaImage};
use puzzle_term::{
    Origin, Session, SessionConfig,
    frame_builder::context::TextContext,
    messages::Command,
    platform::{PtySize, Services, Transport, Waker},
    settings::Settings,
    text::{FontData, font_set::FontSet},
    viewport::ScrollInput,
    vt::input::{Modifiers, MouseEvent, MouseKind},
};
use std::time::Instant;
/// The scripted fixture has no application process or asynchronous input.
pub(super) struct Sink;
impl Transport for Sink {
    /// Cursor reports are discarded without bypassing Session policies.
    fn write(&mut self, _bytes: &[u8]) -> Result<(), String> {
        Ok(())
    }
    /// Fixed fixture dimensions create no operating-system resize.
    fn resize(&mut self, _size: PtySize) -> Result<(), String> {
        Ok(())
    }
    /// There is no child to retain after the evidence finishes.
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}
/// All refresh instants are driven by the scenario itself.
pub(super) struct Manual;
impl Waker for Manual {
    /// A request is fulfilled by the explicit sample below.
    fn request_frame(&mut self) {}
    /// No wall-clock timer runs in this deterministic pixel fixture.
    fn wake_at(&mut self, _at: Instant) {}
}
/// Exercises integral rounding, both edge rows, all grid clips and GPU ink.
pub(super) fn run(state: &mut Scenario) -> Result<(), String> {
    for dpr in [1.0, 2.0] {
        sample(state, dpr)?;
    }
    Ok(())
}
/// Uses the real public command path rather than setting builder geometry.
fn sample(state: &mut Scenario, dpr: f32) -> Result<(), String> {
    // Filesystem access belongs to this native shell; the core receives bytes.
    let bytes = std::fs::read("assets/fonts/JetBrainsMono-Variable.ttf")
        .map_err(|error| error.to_string())?;
    // Only the already-bundled regular face is needed for this ASCII scene.
    let fonts = FontSet::new(FontData::new(bytes, 0), None, None)?;
    let mut context = TextContext::new(fonts, 14.0, dpr)?;
    // Device-cell measurements, rather than guessed font dimensions, define clips.
    let metrics = context.metrics();
    // One injected timestamp excludes wall-clock easing from pixel expectations.
    let now = Instant::now();
    let mut session = scroll_scene(&context, now);
    // Persistent textures must receive only the atlas spans the real frame emits.
    let mut gpu = FrameRenderer::new(&state.renderer, wgpu::TextureFormat::Rgba8Unorm);
    // Initialize uploads before moving; the same device/atlas remains alive.
    let mut drawing = super::drawing::Drawing::default();
    let initial = drawing
        .draw(&mut session, now, &mut context)?
        .ok_or("initial scroll frame absent")?;
    gpu.render_to_image(&state.renderer, initial)
        .map_err(|error| error.to_string())?;
    // The extra fraction tests rounding instead of a conveniently aligned input.
    let half = f64::from(metrics.height_px) / 2.0 + 0.2;
    // User origin enters interceptors; this is not direct model manipulation.
    session.submit(wheel(-half), Origin::User);
    // Pump accepts input now, before a renderer or display turn is consulted.
    session.pump(now);
    // Snapshot/render agree with the presentation range exposed to a host.
    let shift = session.viewport().device_shift();
    // This frame advances the same animation clock used by the live window.
    let image = render_fractional(
        state,
        &mut session,
        &mut context,
        &mut drawing,
        &mut gpu,
        now,
        shift,
    )?;
    // Save only mechanically valid evidence, retaining both hinting paths.
    state.image = Some(image);
    state.preview_ready = true;
    state.screenshot(&format!("fractional-{dpr:.0}x"))
}
/// Authors three colored viewport rows and history with an explicitly fractional policy.
fn scroll_scene(context: &TextContext, now: Instant) -> Session {
    // The grid has three rows, but its primary ring retains older output.
    let mut session = Session::new(
        SessionConfig {
            columns: 12,
            rows: 3,
            ..Default::default()
        },
        Services::new(Box::new(Sink), Box::new(Manual)),
    );
    // Source policy stores measured cells; the diagnostic host owns its row cache.
    session.set_text_context(context);
    // Disable snapping so the fractional endpoint remains available to readback.
    // A unit multiplier makes the requested physical displacement unambiguous.
    session.apply_settings(
        &Settings {
            scrolling_multiplier: 1.0,
            scrolling_snap_to_rows: false,
            ..Default::default()
        },
        now,
    );
    // Alternating paper makes each physical row boundary observable on GPU.
    session.feed(b"\x1b[?25l\x1b[97;41mABCDEFGHIJKL\r\n\x1b[42mABCDEFGHIJKL\r\n\x1b[44mABCDEFGHIJKL\r\n\x1b[41mABCDEFGHIJKL\r\n\x1b[42mABCDEFGHIJKL\r\n\x1b[44mABCDEFGHIJKL", now);
    session
}
/// Captures geometry, submits it, then asserts the two edge strips on GPU.
fn render_fractional(
    state: &Scenario,
    session: &mut Session,
    context: &mut TextContext,
    drawing: &mut super::drawing::Drawing,
    gpu: &mut FrameRenderer,
    now: Instant,
    shift: i32,
) -> Result<RgbaImage, String> {
    let metrics = context.metrics();
    let frame = drawing
        .draw(session, now, context)?
        .ok_or("fractional scroll frame absent")?;
    let width = 12 * metrics.width_px;
    let height = 3 * metrics.height_px;
    // Capturing an extra row must not enlarge the target or its grid scissor.
    if frame.target_size != [width + 32, height + 32] {
        return Err("fractional target height changed".into());
    }
    // Rect, text and cursor layers must all use the grid clip, regardless of ink.
    for batch in &frame.batches {
        let clip = batch.scissor.ok_or("grid batch lacks scissor")?;
        if (clip.x, clip.y, clip.width, clip.height) != (16, 16, width, height) {
            return Err("fractional batch scissor differs from fixed grid".into());
        }
    }
    if frame.slugs.iter().any(|glyph| glyph.t[1].fract() != 0.0) {
        return Err("grid text origin sits on a half device pixel".into());
    }
    // Read actual GPU bytes; CPU geometry alone cannot prove scissor enforcement.
    let (image, _) = gpu
        .render_to_image(&state.renderer, frame)
        .map_err(|error| error.to_string())?;
    check_edges(&image, width, height, metrics.height_px, shift)?;
    Ok(image)
}

/// Padding remains clear; each clipped end retains actual foreground glyph ink.
fn check_edges(
    image: &RgbaImage,
    width: u32,
    height: u32,
    row_height: u32,
    shift: i32,
) -> Result<(), String> {
    // Independent counters reject either missing partially visible text row.
    let mut top_ink = 0;
    let mut bottom_ink = 0;
    // Clip strip heights are computed in device pixels from the measured row.
    let top_end = (row_height as i32 + shift) as u32;
    // The fourth captured row begins after three row advances plus the offset.
    let bottom_start = (3 * row_height) as i32 + shift;
    for y in 0..image.height {
        for x in 0..image.width {
            // RGBA readback is tightly packed; each sample starts at four bytes.
            let at = ((y * image.width + x) * 4) as usize;
            let rgb = &image.pixels[at..at + 3];
            // Include left/right padding as well as the vertically clipped edges.
            let in_grid = x >= 16 && x < 16 + width && y >= 16 && y < 16 + height;
            // A stray dilated glyph or background rectangle must fail immediately.
            if !in_grid && rgb != [14, 18, 26] {
                return Err(format!("scrolled ink escaped scissor at {x},{y}"));
            }
            // Bright near-white ink distinguishes glyphs from ANSI-colored paper.
            if in_grid && rgb.iter().all(|channel| *channel > 220) {
                if y < 16 + top_end {
                    top_ink += 1;
                }
                if y as i32 >= 16 + bottom_start {
                    bottom_ink += 1;
                }
            }
        }
    }
    // White foreground differs from all three saturated row-background colors.
    if top_ink == 0 || bottom_ink == 0 {
        return Err(format!(
            "clipped glyph ink missing: top={top_ink}, bottom={bottom_ink}"
        ));
    }
    Ok(())
}

/// A deterministic pointer hit carries the wheel through the public bus lane.
fn wheel(delta: f64) -> Command {
    Command::Wheel {
        delta: ScrollInput::Pixels(delta),
        // Local scrolling uses no button reports; position still survives replay.
        pointer: MouseEvent {
            button: 64,
            kind: MouseKind::Wheel,
            column: 0,
            row: 0,
            pixel_x: 0,
            pixel_y: 0,
            modifiers: Modifiers(0),
            button_held: false,
            click_count: 0,
        },
    }
}

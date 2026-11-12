//! Kitty golden scenes use public Session bytes and the native worker's codec (M15.3).
//!
//! The scene inputs are authored here, not downloaded images. Real PNG decoding,
//! tracked anchors, publications and the existing raster shader all participate.
//! Exact paper/color assertions run before screenshots become review candidates.
//!
//! Each case owns its Session and text context, so an earlier image cannot hide a
//! missing upload or incorrect source ID in a later case. The renderer device is
//! reused by the scenario, just as the App shares one device across its surfaces.
//! DPR changes only destination cells: sixteen source pixels always remain sixteen
//! source pixels. The reflow case moves a tracked column into the next physical row.
//! Deletion is checked on a second publication, not inferred from the parser table.
//! A rebuilt executor must restore the same decoded pixels without another APC.
use super::{
    Scenario,
    drawing::Drawing,
    scroll::{Manual, Sink},
};
use puzzle_render::{FrameRenderer, RgbaImage};
use puzzle_term::{
    Session, SessionConfig, SessionMessage,
    frame_builder::context::TextContext,
    platform::Services,
    text::{FontData, font_set::FontSet},
};
use std::time::Instant;
/// Payload construction is separate from placement/viewport evidence.
mod payload;
/// Captured real-client byte streams use the same native decoder and sealed draw path.
pub(super) mod replay;
/// Multi-row natural and scaled tiles must match an untiled image at every pixel.
mod seams;
/// Pixel and geometry checks are independent of reference blessing.
mod verify;

/// Every required image case runs with both hinted and unhinted terminal metrics.
pub(super) fn run(state: &mut Scenario) -> Result<(), String> {
    // Both paths use their own font context so hinted ink cannot leak between DPRs.
    for dpr in [1.0, 2.0] {
        // Each name identifies one work-order case and a stable screenshot filename.
        for name in [
            "rgba",
            "png-alpha",
            "negative-z",
            "source-crop",
            "half-viewport",
            "reflow",
        ] {
            sample(state, name, dpr)?;
        }
    }
    // Append new cases so the twelve existing reference names stay stable.
    for dpr in [1.0, 2.0] {
        sample(state, "placeholders", dpr)?;
    }
    // Boundary witnesses append without renumbering any previous candidate crop.
    for dpr in [1.0, 2.0] {
        sample(state, "placeholder-seams", dpr)?;
    }
    Ok(())
}
/// Retained owners match live rendering without requiring a window or child process.
fn sample(state: &mut Scenario, name: &str, dpr: f32) -> Result<(), String> {
    // One clock instant excludes cursor blink or easing from image comparisons.
    let now = Instant::now();
    let (mut session, mut context) = setup(dpr, now)?;
    // Feed the complete wire command before allowing the Session to publish.
    scene(&mut session, name, now)?;
    // Drawing owns the sealed slot and row cache; no live VT query reaches the GPU.
    let mut drawing = Drawing::default();
    let mut gpu = FrameRenderer::new(&state.renderer, wgpu::TextureFormat::Rgba8Unorm);
    let frame = drawing
        .draw(&mut session, now, &mut context)?
        .ok_or("image frame absent")?;
    // Assert anchor geometry before a screenshot could conceal a misplaced image.
    verify::geometry(frame, name, context.metrics())?;
    // The executor consumes glyph uploads; retain this cold fixture copy for recovery.
    let restore_uploads = frame.uploads.clone();
    let (image, _) = gpu
        .render_to_image(&state.renderer, frame)
        .map_err(|e| e.to_string())?;
    // Readback checks are independent of the candidate PNG comparator.
    verify::pixels(&image, name, context.metrics())?;
    // A fresh device executor restores identical pixels from the sealed sources.
    let mut recovered = FrameRenderer::new(&state.renderer, wgpu::TextureFormat::Rgba8Unorm);
    recovered
        .apply_uploads(&state.renderer, &restore_uploads)
        .map_err(|e| e.to_string())?;
    let (restored, _) = recovered
        .render_to_image(&state.renderer, frame)
        .map_err(|e| e.to_string())?;
    // Exact equality checks source recovery, alpha composition and text uploads together.
    if image.pixels != restored.pixels {
        return Err(format!("{name}: recovered image differs"));
    }
    // Successful submission, not construction, commits regions and source scheduling.
    drawing.presented(&mut session, now)?;
    // Deleting sources must remove old GPU ink on the next actual publication.
    // Virtual prototypes have no screen position: only ID/range deletion applies.
    session.feed(delete_command(name), now);
    // Deletion requests another frame even though the decoded source no longer exists.
    let empty = drawing
        .draw(&mut session, now, &mut context)?
        .ok_or("delete frame absent")?;
    if empty
        .images
        .as_ref()
        .is_some_and(|images| !images.sources.is_empty())
    {
        return Err(format!("{name}: deleted source remains visible"));
    }
    // Execute that removal frame so a retained texture cannot stand in for live placement.
    gpu.render_to_image(&state.renderer, empty)
        .map_err(|e| e.to_string())?;
    // Keep the earlier image-bearing pixels as the review artifact after deletion passes.
    // Scenario screenshots own a completed readback, not a mutable GPU attachment.
    state.image = Some(image);
    // Suppress the generic preview so it cannot overwrite the verified image pixels.
    state.preview_ready = true;
    state.screenshot(&format!("{name}-{dpr:.0}x"))
}
/// Creates deterministic terminal/font owners before any image command is fed.
fn setup(dpr: f32, now: Instant) -> Result<(Session, TextContext), String> {
    // Loading the bundled primary face is fixture setup, never timed frame work.
    let bytes =
        std::fs::read("assets/fonts/JetBrainsMono-Variable.ttf").map_err(|e| e.to_string())?;
    // Bundled bytes keep the scene independent of user-installed fallback fonts.
    let context = TextContext::new(
        FontSet::new(FontData::new(bytes, 0), None, None)?,
        14.0,
        dpr,
    )?;
    // A short grid exposes history clipping without a full-window reference image.
    let mut session = Session::new(
        SessionConfig {
            columns: 20,
            rows: 6,
            ..Default::default()
        },
        Services::new(Box::new(Sink), Box::new(Manual)),
    );
    // The owner needs measured cells for cursor policy and placement visibility.
    session.set_text_context(&context);
    // Fixture admission is bounded independently from the native default allowance.
    session.enable_images(1024 * 1024);
    // Hide the caret: its phase must not change the selected source-color samples.
    session.feed(b"\x1b[?25l", now);
    // Return both owners so frame assembly borrows the exact metrics sent to Session.
    Ok((session, context))
}
/// Builds distinct wire/layout cases, including text-backed virtual image tiles.
fn scene(session: &mut Session, name: &str, now: Instant) -> Result<(), String> {
    match name {
        "placeholders" => placeholders(session, now)?,
        "placeholder-seams" => seams::scene(session, now)?,
        "rgba" => payload::transmit(session, "i=1,c=8,r=4", false, now)?,
        "png-alpha" => {
            // Bright ASCII under translucent color makes alpha errors visible.
            session.feed(b"IMAGE THROUGH ALPHA\r\nIMAGE THROUGH ALPHA\x1b[H", now);
            payload::transmit(session, "i=1,c=8,r=4", true, now)?;
        }
        "negative-z" => {
            // Nondefault cell paper hides only the very-low-z image; the next sits above it.
            session.feed(b"\x1b[48;2;0;255;0mMMMM\x1b[0mMMMM\x1b[H", now);
            payload::transmit(session, "i=1,c=8,r=4,z=-2147483648", false, now)?;
            session.feed(b"\x1b[2;1H", now);
            payload::transmit(session, "i=2,c=8,r=4,z=-1", false, now)?;
        }
        "source-crop" => payload::transmit(session, "i=1,x=8,y=0,w=8,h=16,c=4,r=4", false, now)?,
        "half-viewport" => {
            // Two rows of the image enter history while its bottom remains visible.
            payload::transmit(session, "i=1,c=8,r=6", false, now)?;
            session.feed(b"\r\n\r\n\r\n\r\n\r\n\r\n\r\n", now);
        }
        "reflow" => {
            // Printing sixteen cells makes the placement's tracked column wrap to six.
            session.feed(b"abcdefghijklmnop", now);
            payload::transmit(session, "i=1,c=4,r=2", false, now)?;
            // Fill the original logical line so reflow retains all sixteen anchor columns.
            session.feed(b"qrst", now);
            // The host must resize its physical canvas together with the VT columns.
            let cell = session.cell_metrics().ok_or("reflow metrics absent")?;
            session.resize_grid(10, 6, [10 * cell.width_px + 32, 6 * cell.height_px + 32])?;
        }
        _ => return Err("unknown image scene".into()),
    }
    // Consume queued facts before the late publication as an ordinary host does.
    session.pump(now);
    Ok(())
}

/// Virtual prototypes require identity deletion; ordinary scenes delete visible placements.
fn delete_command(name: &str) -> &'static [u8] {
    // Physical deletion must remove both negative-z sources, not just source one.
    if name.starts_with("placeholder") {
        b"\x1b_Ga=d,d=I,i=1\x1b\\"
    } else {
        b"\x1b_Ga=d,d=A\x1b\\"
    }
}

/// Emits the protocol's compact row-only form and two unknown-ID controls.
fn placeholders(session: &mut Session, now: Instant) -> Result<(), String> {
    // The source has four saturated quadrants, so UV tile errors change color.
    // U=1 supplies only a prototype; these later VT cells provide its location.
    payload::transmit(session, "U=1,i=1,p=7,c=4,r=4", false, now)?;
    // These are wire indices zero through three from kitty's frozen table.
    // Explicit CUP separates rows, preventing inheritance across a row boundary.
    for (row, mark) in ['\u{0305}', '\u{030d}', '\u{030e}', '\u{0310}']
        .iter()
        .enumerate()
    {
        // Row-only first cell and unmarked successors exercise left inheritance.
        session.feed(
            format!(
                "\x1b[{};1H\x1b[38;5;1;58;5;7m\u{10eeee}{mark}\u{10eeee}\u{10eeee}\u{10eeee}",
                row + 1
            )
            .as_bytes(),
            now,
        );
    }
    // Unknown source and placement IDs must stay paper, never tofu.
    // The second cell keeps source ID one but requests nonexistent prototype eight.
    session.feed(
        "\x1b[1;6H\x1b[38;5;99m\u{10eeee}\x1b[38;5;1;58;5;8m\u{10eeee}".as_bytes(),
        now,
    );
    // The caller publishes after this function, so no intermediate prototype draws.
    Ok(())
}

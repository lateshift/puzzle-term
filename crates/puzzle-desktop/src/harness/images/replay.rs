//! Real-client preview captures replay through Session, native decode and GPU draws.
//!
//! Captures stop before the client's quit/delete stream. Their exact bytes remain
//! under target until the user approves neutral recordings for assets (§13.4).
//! A second submission without image batches proves that the first actually drew
//! image ink; protocol metadata alone cannot satisfy this regression witness.
//!
//! This command is diagnostic, not a new transport or host API. It consumes bytes
//! already recorded from a real client's PTY, including its queries and CUP puts.
//! It never synthesizes replies to influence the client's already captured choice.
//! Probe-order correctness is checked independently on the real session worker.
//! The capture's replies/events remain beside the raw stream for attribution.
//!
//! Each invocation owns a fresh Session and text context, excluding old image IDs
//! or warmed source textures from satisfying a missing transmission. A source's
//! asynchronous decode barrier still separates earlier queries from later text.
//! The native codec is called synchronously here because a replay has no child
//! waiting on replies and no GUI thread that could be stalled by a PNG decode.
//!
//! Source dimensions come from APC; destination dimensions come from the captured
//! grid and font geometry. Replacing either with guessed preview dimensions would
//! conceal source-crop and placement errors in real per-cell KgpOld commands.
//! The two readbacks share their renderer and immutable source allocations. Only
//! image batches are removed for the control, so glyphs and backgrounds cancel.
//! A zero difference therefore fails even when the source table is populated.
//! Screenshots retain the original image-bearing submission for human review.
use super::*;

/// Replays one captured preview at the scenario's recorded grid and DPR.
pub(in crate::harness) fn run(state: &mut Scenario, path: Option<&str>) -> Result<(), String> {
    // The scenario names a local capture explicitly; there is no default user file.
    let bytes = std::fs::read(path.ok_or("image-replay needs a preview capture")?)
        .map_err(|e| e.to_string())?;
    let now = Instant::now();
    let (mut session, mut context) = setup(state, now)?;
    feed(&mut session, &bytes, now)?;
    // Publication seals every source before GPU assembly, as in the app's render actor.
    let mut drawing = Drawing::default();
    let frame = drawing
        .draw(&mut session, now, &mut context)?
        .ok_or("no replay frame")?;
    let sources = frame
        .images
        .as_ref()
        .map_or(0, |images| images.sources.len());
    let placements = session
        .images()
        .map_or(0, |images| images.placements().len());
    if sources == 0 {
        // Include protocol table size to distinguish invisible puts from no puts.
        return Err(format!(
            "client preview has no visible image ({placements} placements)"
        ));
    }
    let mut gpu = FrameRenderer::new(&state.renderer, wgpu::TextureFormat::Rgba8Unorm);
    let (image, _) = gpu
        .render_to_image(&state.renderer, frame)
        .map_err(|e| e.to_string())?;
    // Removing only image draws preserves text, backgrounds, crops and alpha blend.
    frame
        .batches
        .retain(|batch| !matches!(batch.pipeline, puzzle_term::frame::PipelineKind::Image(_)));
    let (paper, _) = gpu
        .render_to_image(&state.renderer, frame)
        .map_err(|e| e.to_string())?;
    let changed = image
        .pixels
        .chunks_exact(4)
        .zip(paper.pixels.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    if changed == 0 {
        return Err("visible client images produced no GPU ink".into());
    }
    // Persist counts beside the screenshot so the user can inspect both protocols.
    std::fs::write(
        state.output.join("image-replay.txt"),
        format!(
            "bytes={} sources={sources} placements={placements} changed_pixels={changed}\n",
            bytes.len()
        ),
    )
    .map_err(|e| e.to_string())?;
    state.image = Some(image);
    // Prevent the generic clear/text screenshot producer from replacing this readback.
    state.preview_ready = true;
    state.screenshot("client-preview")
}

/// Captured app metrics use its 15-point font, not the older 14-point golden scenes.
fn setup(state: &Scenario, now: Instant) -> Result<(Session, TextContext), String> {
    // Bundled fonts remove system fallback variation from the image geometry check.
    let bytes =
        std::fs::read("assets/fonts/JetBrainsMono-Variable.ttf").map_err(|e| e.to_string())?;
    let context = TextContext::new(
        FontSet::new(FontData::new(bytes, 0), None, None)?,
        15.0,
        state.dpr,
    )?;
    let mut session = Session::new(
        SessionConfig {
            columns: state.cols as usize,
            rows: state.rows as usize,
            ..Default::default()
        },
        Services::new(Box::new(Sink), Box::new(Manual)),
    );
    // Public geometry setup matches an ordinary surface before its child starts.
    session.set_text_context(&context);
    let cell = context.metrics();
    // Padding remains the harness default; cell dimensions are already device pixels.
    session.resize_grid(
        state.cols as usize,
        state.rows as usize,
        [
            state.cols * cell.width_px + 32,
            state.rows * cell.height_px + 32,
        ],
    )?;
    session.enable_images(320 * 1024 * 1024);
    // Caret blinking is unrelated to client image ink and must not alter the control.
    session.feed(b"\x1b[?25l", now);
    Ok((session, context))
}

/// Decoder results release earlier reply barriers before any later captured bytes.
fn feed(session: &mut Session, bytes: &[u8], now: Instant) -> Result<(), String> {
    // Decode barriers retain suffixes, including DA1 and subsequent per-cell puts.
    // Empty feeds drain them only after the earlier native result has been applied.
    session.feed(bytes, now);
    loop {
        if let Some(request) = session.take_image_decode_request() {
            // Use the shipped bounded codec, including compression and RGB expansion.
            session.apply(
                SessionMessage::ImageDecoded(puzzle_embed::diagnostics::decode_image(request)),
                now,
            )?;
        }
        if session.feed(&[], now).pending_bytes == 0 {
            // An empty pending queue proves every captured placement reached the owner.
            break;
        }
    }
    Ok(())
}

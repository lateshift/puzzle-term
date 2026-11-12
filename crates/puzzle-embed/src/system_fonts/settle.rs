//! Evidence helpers build one complete native frame, with no completion polling.
//!
//! D-41 makes the first build authoritative. Keeping these shared entry points
//! lets existing screenshot and allocation probes verify the ordinary renderer.
use puzzle_term::{
    Origin, Session,
    frame::RenderFrame,
    frame_builder::{FrameBuilder, context::TextContext},
    frame_snapshot::FrameSnapshot,
    messages::Command,
    vt::terminal::Terminal,
};
use std::time::Instant;

/// Builds the first terminal frame with complete synchronous source ink.
pub fn terminal(
    context: &mut TextContext,
    builder: &mut FrameBuilder,
    vt: &Terminal,
    frame: &mut RenderFrame,
) -> Result<(), String> {
    // A complete synchronous source returns final owned ink during this same build.
    // No delay or second frame can turn a provisional screenshot into this witness.
    builder.build(context, vt, frame)
}

/// Uses exactly one immutable captured snapshot without querying the source owner.
pub fn snapshot(
    context: &mut TextContext,
    builder: &mut FrameBuilder,
    snapshot: &FrameSnapshot,
    frame: &mut RenderFrame,
) -> Result<(), String> {
    // Reclamation precedes collection of this frame's stable atlas addresses.
    context.begin_frame()?;
    // Shape only immutable captured rows; later parser edits cannot enter this build.
    // Preparation and uploads belong to this first frame rather than a settling tail.
    builder.build_snapshot(context, snapshot, frame)
}

/// Includes provider layout and transformed sizes in one sealed publication.
pub fn session(
    session: &mut Session,
    context: &mut TextContext,
) -> Result<crate::drawing::Drawing, String> {
    // Request a normal frame even when an earlier fixture already consumed damage.
    session.submit(Command::GlyphRasterized, Origin::Platform);
    session.pump(Instant::now());
    // Providers share this exact preparation path, including bitmap-only transformed ink.
    let mut drawing = crate::drawing::Drawing::default();
    drawing
        .draw(session, Instant::now(), context)?
        .ok_or("probe frame absent")?;
    // Return the drawing owner because the caller submits this exact completed frame.
    // Dropping it here would lose its pending glyph uploads and retained row cache.
    Ok(drawing)
}

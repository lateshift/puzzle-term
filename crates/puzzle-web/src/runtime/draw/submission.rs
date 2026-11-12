//! The canvas painter consumes uploads only when a built image reaches the GPU.
//!
//! Session publication and actual surface submission have different lifetimes.
//! In particular, a skipped drawable must preserve both pending ink and the
//! regions of the last presented image; only successful submission acknowledges.
use super::*;

/// Submits the host-owned frame and then returns its visible hit table to Session.
pub(super) fn submit(
    runtime: &mut Runtime,
    text: &resources::Text,
    renderer: &puzzle_render::Renderer,
    glyph_budget: usize,
) -> Result<(), JsValue> {
    // Drawing storage and the surface are disjoint permanent host fields.
    let frame = &mut runtime.render.frame;
    let surface = runtime
        .surface
        .as_ref()
        .ok_or_else(|| value::error("NotRendered", "surface missing"))?;
    // A retained painter owns atlas uploads across sessions and frames;
    // replacing it per canvas would lose previously prepared text.
    let mut painter = text.painter.borrow_mut();
    if painter.is_none() {
        *painter = Some(puzzle_render::FrameRenderer::new(
            renderer,
            renderer
                .surface_format(surface, frame.target_size[0], frame.target_size[1])
                .map_err(|e| value::error("WebGpuUnavailable", &e.to_string()))?,
        ));
    }
    // Text-resource identity includes the per-atlas memory policy. Each new
    // resource starts with fresh handles and a fresh painter, preserving sharing.
    // Frame building already borrows the CPU context; receive its policy by value.
    if let Some(painter) = painter.as_mut() {
        painter.set_glyph_budget(renderer, glyph_budget);
    }
    // Surface errors leave upload ownership in the retained painter/frame.
    // A later successful submission can reuse prepared glyphs without blank ink.
    painter
        .as_mut()
        .ok_or_else(|| value::error("WebGpuUnavailable", "pipeline missing"))?
        .render_surface(renderer, frame, surface)
        .map_err(|e| value::error("WebGpuUnavailable", &e.to_string()))?;
    // Both halves run on this thread, but a build is not a presentation.
    runtime
        .frame_presented()
        .map_err(|e| value::error("InvalidArgument", &e))?;
    Ok(())
}

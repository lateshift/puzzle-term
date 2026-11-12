//! Two independent terminal grids share one text context (D-30).
//!
//! The GPU judge draws both warm sessions through one retained executor,
//! then draws the same text through an isolated context and executor. The
//! comparison catches a reused atlas handle whose texels were never sent
//! to the shared GPU texture. An explicit atlas reset must force both row
//! caches to rebuild even though neither terminal printed another byte.
//!
//! Two VT instances deliberately print the same short line. Identical
//! source glyphs make a repeated preparation visible as an increased atlas
//! entry count; different source text could legitimately grow that count.
//! Their row identities are independent, so reusing one row cache for both
//! would be incorrect even if a screenshot happened to match.
//!
//! The cold renderer owns an independent GPU texture. It receives a full
//! first-frame upload and therefore judges whether the shared executor's
//! second frame can rely on glyph texels installed by the first. Comparing
//! CPU instances alone would miss a missing or misordered GPU upload.
//! A reset simulates eviction or device recovery without modifying VT;
//! equality afterward proves a generation change invalidates both caches.

use super::{Scenario, text};
use puzzle_render::{FrameRenderer, Renderer, RgbaImage};
use puzzle_term::text::font_set::FontSet;
use puzzle_term::vt::Terminal;
use puzzle_term::{
    frame::RenderFrame,
    frame_builder::{FrameBuilder, context::TextContext},
};

/// Loads approved font bytes only at the native harness boundary.
fn fonts() -> Result<FontSet, String> {
    // All paths are repository assets read by the native harness. The
    // terminal and web libraries contain neither file IO nor embedded TTFs.
    FontSet::new(
        text::read_font("assets/fonts/JetBrainsMono-Variable.ttf")?,
        Some(text::read_font(
            "assets/fonts/JetBrainsMono-Italic-Variable.ttf",
        )?),
        Some(text::read_font(
            "assets/fonts/SymbolsNerdFontMono-Regular.ttf",
        )?),
    )
}

/// Creates a fresh screen without borrowing either session's row identity.
fn terminal() -> Terminal {
    // The arrow may form a ligature, while repeated letters expose glyph
    // deduplication across independently allocated terminal rows.
    let mut term = Terminal::new(12, 1, 0, 0);
    term.advance(b"Atlas AB ->");
    term
}

/// Renders one builder while preserving the executor's uploaded atlas.
fn draw(
    renderer: &Renderer,
    executor: &mut FrameRenderer,
    builder: &mut FrameBuilder,
    context: &mut TextContext,
    term: &Terminal,
) -> Result<RgbaImage, String> {
    // A fresh instance array is fine: the test targets persistent atlas
    // contents and per-session row caches, not RenderFrame Vec capacity.
    let mut frame = RenderFrame::default();
    builder.build(context, term, &mut frame)?;
    executor
        .render_to_image(renderer, &mut frame)
        .map(|(image, _)| image)
        .map_err(|error| error.to_string())
}

/// Requires identical pixels, including transparent texture upload slots.
fn equal(actual: &RgbaImage, expected: &RgbaImage, label: &str) -> Result<(), String> {
    // Both images use one Rgba8Unorm device and the same physical metrics;
    // exact bytes are a stronger judge than a color-distance threshold.
    if actual.width == expected.width
        && actual.height == expected.height
        && actual.pixels == expected.pixels
    {
        Ok(())
    } else {
        Err(format!(
            "{label}: shared context pixels differ from cold context"
        ))
    }
}

/// Runs prepared-once and generation-reset checks and retains a review PNG.
pub(super) fn run(state: &mut Scenario) -> Result<(), String> {
    // Host ownership is visible here: the second builder receives only a
    // shared borrow and performs no second font read or atlas creation.
    let mut shared = TextContext::new(fonts()?, 14.0, state.dpr)?;
    let mut first = FrameBuilder::new(&shared);
    let mut second = FrameBuilder::new(&shared);
    let (term_a, term_b) = (terminal(), terminal());
    // One executor must retain the first frame's texture upload before
    // the second frame arrives without that glyph in its upload list.
    let mut gpu = FrameRenderer::new(&state.renderer, wgpu::TextureFormat::Rgba8Unorm);
    let first_image = draw(&state.renderer, &mut gpu, &mut first, &mut shared, &term_a)?;
    let prepared = shared.prepared_glyphs();
    // A zero count would make a blank screenshot compare equal while
    // proving none of the sharing machinery had been exercised.
    if prepared == 0 {
        return Err("shared context prepared no glyphs".into());
    }
    let second_image = draw(&state.renderer, &mut gpu, &mut second, &mut shared, &term_b)?;
    // Preparing the same glyph twice means the context was copied rather
    // than shared, even if the independent atlases painted equal pixels.
    if shared.prepared_glyphs() != prepared {
        return Err("second session duplicated prepared glyphs".into());
    }
    // A separately loaded face and empty atlas provide a cold pixel judge.
    // The second shared builder was constructed without another font read.
    let mut isolated = TextContext::new(fonts()?, 14.0, state.dpr)?;
    let mut cold_builder = FrameBuilder::new(&isolated);
    let mut cold_gpu = FrameRenderer::new(&state.renderer, wgpu::TextureFormat::Rgba8Unorm);
    let cold = draw(
        &state.renderer,
        &mut cold_gpu,
        &mut cold_builder,
        &mut isolated,
        &term_a,
    )?;
    equal(&first_image, &cold, "first session")?;
    equal(&second_image, &cold, "second session")?;
    // Eviction changes the context's generation but not either VT row's
    // version. Both builders must request fresh row copies on their turn.
    shared.reset_atlas();
    let reset_a = draw(&state.renderer, &mut gpu, &mut first, &mut shared, &term_a)?;
    if first.rebuilt_rows() != 1 {
        return Err("first session retained an old atlas handle".into());
    }
    let reset_b = draw(&state.renderer, &mut gpu, &mut second, &mut shared, &term_b)?;
    if second.rebuilt_rows() != 1 {
        return Err("second session retained an old atlas handle".into());
    }
    equal(&reset_a, &cold, "first generation reset")?;
    equal(&reset_b, &cold, "second generation reset")?;
    // Retain the second, post-reset image for a reviewer. The assertions
    // above remain the mechanical criterion; visual review is separate.
    state.image = Some(reset_b);
    state.preview_ready = true;
    Ok(())
}

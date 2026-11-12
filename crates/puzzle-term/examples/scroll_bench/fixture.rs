//! Real bundled ASCII warms outlines and both row banks before measured history misses.
use super::*;

/// Context, cache, VT and output owners are reused across every release trial.
pub(super) type Fixture = (TextContext, FrameBuilder, Terminal, RenderFrame);

/// Builds the full 240×70 live grid over 140 untouched history rows at one scale.
pub(super) fn new(dpr: f32) -> Result<Fixture, String> {
    // Only this native example reads files; the neutral builder receives font bytes.
    let bytes =
        std::fs::read("assets/fonts/JetBrainsMono-Variable.ttf").map_err(|e| e.to_string())?;
    let fonts = FontSet::new(FontData::new(bytes, 0), None, None)?;
    // Each scale owns a fresh context, so hinted and unhinted trials cannot share ink.
    let mut context = TextContext::new(fonts, 14.0, dpr)?;
    if std::env::var_os("PUZZLE_GRID_BITMAP_BENCH").is_some() {
        // Only the opt-in native probe selects D-47; existing vector acceptance stays unchanged.
        context.set_grid_renderer(puzzle_term::text::glyph::RendererChoice::Auto);
    }
    // The builder keeps geometry scratch; the frame retains its instance capacities.
    let mut builder = FrameBuilder::new(&context);
    let mut frame = RenderFrame::default();
    // Retention cannot truncate a trial or move it into a blank document boundary.
    let mut terminal = Terminal::new(240, 70, 200, 2_000_000);
    let mut text = Vec::with_capacity(210 * 242);
    for row in 0..210 {
        // Every letter needed by history is already present in the first live frame.
        text.extend(std::iter::repeat_n(b'A' + row as u8 % 26, 240));
        if row < 209 {
            text.extend_from_slice(b"\r\n");
        }
    }
    // Hiding the cursor keeps every trial to one content miss, without ligature isolation.
    text.extend_from_slice(b"\x1b[?25l");
    terminal.advance(&text);
    // Initial outline extraction and spare-bank reservation precede all trial clocks.
    builder.build(&mut context, &terminal, &mut frame)?;
    // Simulate successful consumption so pending startup writes do not enter scroll work.
    frame.uploads.clear();
    Ok((context, builder, terminal, frame))
}

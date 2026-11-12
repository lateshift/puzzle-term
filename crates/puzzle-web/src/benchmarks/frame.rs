//! Warm full-screen and changed-row CPU assembly, excluding parse and GPU work.
//!
//! This mirrors the native frame_bench example: same face, DPR, 240×70 grid,
//! warm glyphs and epoch invalidation. A row edit stays outside its timer.
use puzzle_term::{
    frame::RenderFrame,
    frame_builder::{FrameBuilder, context::TextContext},
    text::{FontData, font_set::FontSet},
    vt::Terminal,
};
use std::hint::black_box;
use wasm_bindgen::prelude::*;
use web_time::Instant;

/// Warms glyphs and row caches before measuring either CPU budget.
fn fixture() -> Result<(TextContext, FrameBuilder, Terminal, RenderFrame), String> {
    // Reuse the approved primary face; no network fetch enters the measurement.
    let bytes = include_bytes!("../../../../assets/fonts/JetBrainsMono-Variable.ttf");
    // One regular face is sufficient for the specified ASCII redraw workload.
    let fonts = FontSet::new(FontData::new(bytes.to_vec(), 0), None, None)?;
    // DPR 1 selects the same small-text hinting regime as the native probe.
    let mut context = TextContext::new(fonts, 14.0, 1.0)?;
    let mut builder = FrameBuilder::new(&context);
    // No scrollback is needed: every visible cell belongs to this fixed screen.
    let mut terminal = Terminal::new(240, 70, 0, 0);
    // Allocate the fixture before timing; row construction is not frame assembly.
    let mut text = Vec::with_capacity(70 * 242);
    for row in 0..70 {
        // Vary letters by row so the warm cache includes several real outlines.
        text.extend(std::iter::repeat_n(b'A' + row as u8 % 26, 240));
        // Explicit CR/LF starts the next row without an extra bottom scroll.
        if row < 69 {
            text.extend_from_slice(b"\r\n");
        }
    }
    // Feed all cells once before asking the builder to warm its row cache.
    terminal.advance(&text);
    let mut frame = RenderFrame::default();
    // The first extraction/layout is startup cost, not warm redraw cost.
    builder.build(&mut context, &terminal, &mut frame)?;
    Ok((context, builder, terminal, frame))
}

/// Returns full redraw and one-row medians, in milliseconds, in that order.
/// The only exported binding is in the explicit development benchmark build.
#[cfg_attr(feature = "measurements", wasm_bindgen(js_name = benchmarkFrame))]
pub fn measure() -> Result<Vec<f64>, JsValue> {
    run()
        .map(|(full, row)| vec![full, row])
        .map_err(|e| js_sys::Error::new(&e).into())
}

/// Timed regions contain only assembly; invalidation and editing stay outside.
fn run() -> Result<(f64, f64), String> {
    let (mut context, mut builder, mut terminal, mut frame) = fixture()?;
    // Odd sample counts give an observed median rather than interpolated timing.
    let mut full = [0.0; 21];
    for sample in &mut full {
        // Rebuild all instances without forcing expensive font extraction again.
        builder.set_context(&context);
        let start = Instant::now();
        builder.build(&mut context, &terminal, &mut frame)?;
        // Convert seconds to milliseconds to compare directly with design §15.2.
        *sample = start.elapsed().as_secs_f64() * 1000.0;
        black_box(frame.slugs.len());
    }
    // The row series has more samples because each incremental build is shorter.
    let mut row = [0.0; 101];
    for (trial, sample) in row.iter_mut().enumerate() {
        // Alternating bytes invalidate exactly one row through the real VT path.
        terminal.advance(&[0x1b, b'[', b'H', if trial % 2 == 0 { b'X' } else { b'Y' }]);
        let start = Instant::now();
        builder.build(&mut context, &terminal, &mut frame)?;
        *sample = start.elapsed().as_secs_f64() * 1000.0;
        black_box(frame.slugs.len());
    }
    // Sorting happens after both loops, outside every measured assembly span.
    full.sort_by(f64::total_cmp);
    row.sort_by(f64::total_cmp);
    Ok((full[10], row[50]))
}

/// Measures a newly exposed full-width row plus fractional assembly of 71 rows.
/// Setup and scrolling arithmetic stay outside the timed build, as in §15.2.
#[cfg_attr(feature = "measurements", wasm_bindgen(js_name = benchmarkScroll))]
pub fn scroll() -> Result<f64, JsValue> {
    scroll_run().map_err(|error| js_sys::Error::new(&error).into())
}

/// Moves through real history, rather than timing an unchanged live viewport.
fn scroll_run() -> Result<f64, String> {
    let (mut context, mut builder, _, mut frame) = fixture()?;
    // History is deliberately larger than the 101 newly exposed rows.
    // The byte limit holds all 210 rows without evicting the measurement tail.
    let mut terminal = Terminal::new(240, 70, 200, 2_000_000);
    let mut text = Vec::with_capacity(210 * 242);
    for row in 0..210 {
        // Repeated warmed letters isolate frame assembly from outline startup.
        text.extend(std::iter::repeat_n(b'A' + row as u8 % 26, 240));
        if row < 209 {
            text.extend_from_slice(b"\r\n");
        }
    }
    terminal.advance(&text);
    builder.build(&mut context, &terminal, &mut frame)?;
    // Both edge rows participate, while the backing height stays at 70 rows.
    // An integral half-row offset excludes raster blur from CPU measurements.
    let shift = -(context.metrics().height_px as i32 / 2);
    // A fixed array needs no allocation inside the timed loop.
    let mut samples = [0.0; 101];
    for (step, sample) in samples.iter_mut().enumerate() {
        // Each step exposes one older row; this is not a repeated cache-only frame.
        let top = terminal.screen().ring.scrollback_len() - step - 1;
        let started = Instant::now();
        builder.build_scrolled(&mut context, &terminal, top, shift, &mut frame)?;
        *sample = started.elapsed().as_secs_f64() * 1000.0;
        black_box(frame.slugs.len());
    }
    // Keep every observation; the odd median reduces scheduling noise.
    samples.sort_by(f64::total_cmp);
    Ok(samples[50])
}

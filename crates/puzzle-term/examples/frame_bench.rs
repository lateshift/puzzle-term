//! Native M6.3 frame CPU budget probe over a 240×70 ASCII terminal.
//!
//! The bundled TTF is read at the example boundary, never embedded in a
//! platform-neutral library. Warmed Slug shapes remain cached across trials.
//! A full redraw invalidates the style epoch without changing font size;
//! a one-row trial edits only the first row before the timer starts. The
//! timed region is exactly `FrameBuilder::build`, excluding fixture setup.
//!
//! The release profile matches the native §15.2 frame CPU budgets. A
//! median limits the effect of occasional OS scheduling interruptions.
use puzzle_frame::RenderFrame;
use puzzle_term::frame_builder::{FrameBuilder, context::TextContext};
use puzzle_text::{FontData, font_set::FontSet};
use puzzle_vt::terminal::Terminal;
use std::{hint::black_box, io::Write, time::Instant};
/// Opt-in stage diagnostics run after, and separately from, the budget measurement.
#[path = "frame_bench/diagnostics.rs"]
mod diagnostics;

/// Loads the same primary face used by native terminal frames.
fn fixture() -> Result<(TextContext, FrameBuilder, Terminal, RenderFrame), String> {
    let bytes = std::fs::read("assets/fonts/JetBrainsMono-Variable.ttf")
        .map_err(|error| format!("font fixture: {error}"))?;
    let fonts = FontSet::new(FontData::new(bytes, 0), None, None)?;
    // The renderer never sees a system font in this budget probe, keeping
    // glyph coverage and metrics deterministic across developer machines.
    let context = TextContext::new(fonts, 14.0, 1.0)?;
    let builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(240, 70, 0, 0);
    // A complete screen has the same ASCII run length in each row; CRLF
    // prevents delayed-wrap semantics from creating an extra blank line.
    let mut bytes = Vec::with_capacity(70 * 242);
    for row in 0..70 {
        bytes.extend(std::iter::repeat_n(b'A' + row as u8 % 26, 240));
        if row < 69 {
            bytes.extend_from_slice(b"\r\n");
        }
    }
    terminal.advance(&bytes);
    Ok((context, builder, terminal, RenderFrame::default()))
}

/// Times one complete cache invalidation and many single-row edits.
fn run() -> Result<(), String> {
    let (mut context, mut builder, mut terminal, mut frame) = fixture()?;
    builder.build(&mut context, &terminal, &mut frame)?;
    let mut full = Vec::with_capacity(21);
    for _ in 0..21 {
        // The epoch changes resolved instances, but each glyph outline is
        // already prepared; this measures a warm full-screen redraw.
        builder.set_context(&context);
        let start = Instant::now();
        builder.build(&mut context, &terminal, &mut frame)?;
        full.push(start.elapsed().as_secs_f64() * 1_000_000.0);
        black_box(frame.slugs.len());
    }
    let mut one_row = Vec::with_capacity(101);
    for trial in 0..101 {
        // The new first-cell byte bumps one VT row version. Editing and
        // escape parsing remain outside the frame-build budget.
        let byte = if trial % 2 == 0 { b'X' } else { b'Y' };
        terminal.advance(&[b'\x1b', b'[', b'H', byte]);
        let start = Instant::now();
        builder.build(&mut context, &terminal, &mut frame)?;
        one_row.push(start.elapsed().as_secs_f64() * 1_000_000.0);
        black_box(frame.slugs.len());
    }
    if std::env::args().any(|arg| arg == "--stages") {
        // Keep chronology before sorting; stage clocks never affect these samples.
        diagnostics::samples(&full, &one_row)?;
    }
    full.sort_by(f64::total_cmp);
    one_row.sort_by(f64::total_cmp);
    // A labelled pair gives xtask a stable machine-readable measurement
    // without including fixture construction or formatting in the timer.
    std::io::stdout()
        .write_all(format!("full_us={:.3} row_us={:.3}\n", full[10], one_row[50]).as_bytes())
        .map_err(|error| format!("benchmark output: {error}"))?;
    Ok(())
}

/// Exits unsuccessfully if any font or frame step cannot be measured.
fn main() -> Result<(), String> {
    run()?;
    if std::env::args().any(|arg| arg == "--stages") {
        diagnostics::run()?;
    }
    Ok(())
}

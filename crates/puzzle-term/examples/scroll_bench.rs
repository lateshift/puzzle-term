//! M8.2 scrolling CPU probe: 240×70 plus both clipped edges and one new row.
//!
//! This mirrors the Chrome workload. Fixture parsing, input arithmetic and
//! formatting stay outside the timed frame build. Every sample moves into
//! previously unprepared history, so a zero-damage live frame cannot pass.
//!
//! The 210-row document contains 140 history rows and a 70-row live grid.
//! Starting at the live bottom prepares every letter used later. Scrolling
//! back exposes a new row on every sample while reusing the other 70 caches.
//! The half-row displacement draws 71 physical rows into the fixed grid clip.
//! This represents the incremental workload budgeted as one changed row,
//! rather than a full style invalidation or an unchanged-view cache hit.
use puzzle_term::{
    frame::RenderFrame,
    frame_builder::{FrameBuilder, context::TextContext},
    text::{FontData, font_set::FontSet},
    vt::Terminal,
};
use std::{hint::black_box, io::Write, time::Instant};
/// Records one newly exposed row per trial, including integral/fractional transitions.
fn run_at_scale(dpr: f32, csv: &mut String) -> Result<f64, String> {
    let (mut context, mut builder, terminal, mut frame) = fixture::new(dpr)?;
    let shift = -(context.metrics().height_px as i32 / 2);
    let prepared = context.prepared_glyphs();
    let mut samples = [0.0; 101];
    for (step, sample) in samples.iter_mut().enumerate() {
        // Every step enters untouched history; alternating edges reproduces bank churn.
        let top = terminal.screen().ring.scrollback_len() - step - 1;
        let offset = if step % 2 == 0 { shift } else { 0 };
        let started = Instant::now();
        builder.build_scrolled(&mut context, &terminal, top, offset, &mut frame)?;
        *sample = started.elapsed().as_secs_f64() * 1000.0;
        black_box(frame.slugs.len());
        // Reject a cache-only or cold-glyph trial instead of silently timing another workload.
        if builder.rebuilt_rows() != 1 || context.prepared_glyphs() != prepared {
            return Err("scroll trial must expose exactly one row with warmed glyphs".into());
        }
        // Formatting and evidence growth stay outside every measured build.
        csv.push_str(&format!("{dpr},{step},{top},{offset},{sample:.6},1\n"));
        frame.uploads.clear();
    }
    // Preserve all raw trials; the existing M8 median verdict remains unchanged.
    samples.sort_by(f64::total_cmp);
    Ok(samples[50])
}

/// Both display scales exercise the same cache policy and the unchanged 0.2-ms ceiling.
fn run() -> Result<f64, String> {
    let mut csv = String::from("dpr,sample,top,shift,frame_ms,rebuilt_rows\n");
    let first = run_at_scale(1.0, &mut csv)?;
    let second = run_at_scale(2.0, &mut csv)?;
    // Raw evidence includes the first exposed row and every subsequent bank transition.
    let out = evidence_dir();
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    std::fs::write(format!("{out}/exposed-row-cpu.csv"), csv).map_err(|e| e.to_string())?;
    // Match the live render-thread boundary too, retaining and checking its worst trial.
    build_budget::run()?;
    Ok(first.max(second))
}

/// The render-thread build boundary excludes the separately budgeted snapshot copy.
#[path = "scroll_bench/build_budget.rs"]
mod build_budget;
/// Filesystem/setup work stays outside the newly exposed row's CPU timer.
#[path = "scroll_bench/fixture.rs"]
mod fixture;

/// Propagates setup/build errors rather than measuring an empty frame.
fn main() -> Result<(), String> {
    let millis = run()?;
    std::io::stdout()
        .write_all(format!("scroll_ms={millis:.6}\n").as_bytes())
        .map_err(|error| error.to_string())
}

/// Bitmap trials retain independent evidence rather than overwriting the M16.3 cohort.
fn evidence_dir() -> &'static str {
    if std::env::var_os("PUZZLE_GRID_BITMAP_BENCH").is_some() {
        "target/m16-4"
    } else {
        "target/m16-3"
    }
}

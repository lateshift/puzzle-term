//! Opt-in ASCII frame investigation; the ordinary budget probe has no clocks here.
//!
//! Coarse phases and existing row counters partition the same warmed construction.
//! Row counters add observation overhead, so the unobserved samples remain the
//! acceptance measurement. Snapshot copies and begin-frame work are timed separately.
//! No GPU or surface exists: display scheduling cannot enter this CPU-only probe.
//!
//! The publication is captured explicitly to expose its copy cost. Its epoch
//! matches the builder, exactly as the direct VT convenience path does. The
//! observer sees each completed phase, so adjacent timestamps cover all work
//! rather than summing overlapping spans. The row clocks subdivide the Rows
//! phase; they must not be added again to the six coarse construction phases.
//!
//! All output is outside the measured span. CSV rows preserve trial order and
//! units; medians can be recomputed without discarding scheduler interruptions.
//! Existing cache counters are enabled with row clocks, leaving normal product
//! execution untouched. This is a diagnostic example, never a host API or an
//! alternate frame algorithm. The fixed trial count matches the budget probe.
use puzzle_term::frame_snapshot::FrameSnapshot;
use std::{hint::black_box, io::Write, time::Instant};

/// Retains every unobserved trial before the budget probe sorts its medians.
pub(super) fn samples(full: &[f64], row: &[f64]) -> Result<(), String> {
    let mut out = std::io::stdout().lock();
    for (label, values) in [("full", full), ("row", row)] {
        for (index, value) in values.iter().enumerate() {
            writeln!(out, "sample,{label},{index},{value:.6}").map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Runs separately observed full rebuilds after warming glyphs and output capacities.
pub(super) fn run() -> Result<(), String> {
    let (mut context, mut builder, terminal, mut frame) = super::fixture()?;
    let mut snapshot = FrameSnapshot::default();
    builder.build(&mut context, &terminal, &mut frame)?;
    snapshot.capture(&terminal, builder.style_epoch())?;
    // Column names state units; all phases include their actual runtime branches.
    let mut out = std::io::stdout().lock();
    writeln!(out, "stage,trial,begin_us,snapshot_us,cache_us,rows_us,assembly_us,geometry_us,overlay_us,uploads_us,metrics_us,fallback_us,shape_us,glyph_us")
        .map_err(|e| e.to_string())?;
    for trial in 0..21 {
        // Invalidate resolved row instances without discarding any prepared outline.
        builder.set_context(&context);
        // Reset only diagnostic counters; all glyph and shaping caches stay warm.
        context.measure_rows();
        let before = context.cache_metrics();
        let prepared = context.prepared_glyphs();
        let start = Instant::now();
        context.begin_frame()?;
        builder.sync_generation(context.atlas_generation());
        let begin = start.elapsed().as_secs_f64() * 1e6;
        let start = Instant::now();
        snapshot.capture(&terminal, builder.style_epoch())?;
        let copy = start.elapsed().as_secs_f64() * 1e6;
        let mut previous = Instant::now();
        // Stack storage avoids introducing allocations into the observed builder.
        let mut phases = [0.0; 6];
        builder.build_snapshot_observed(&mut context, &snapshot, &mut frame, |phase| {
            let now = Instant::now();
            phases[phase as usize] = now.duration_since(previous).as_secs_f64() * 1e6;
            previous = now;
        })?;
        let rows = context
            .row_metrics()
            .unwrap_or_default()
            .map(|ns| ns as f64 / 1000.0);
        writeln!(out, "stage,{trial},{begin:.6},{copy:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6}",
            phases[0], phases[1], phases[2], phases[3], phases[4], phases[5], rows[0], rows[1], rows[2], rows[3])
            .map_err(|e| e.to_string())?;
        // Cache deltas explain whether the glyph phase prepares ink or just looks it up.
        for (layer, (after, before)) in context.cache_metrics().into_iter().zip(before).enumerate()
        {
            let delta = std::array::from_fn::<_, 6, _>(|index| after[index] - before[index]);
            writeln!(out, "cache,{trial},{layer},{delta:?}").map_err(|e| e.to_string())?;
        }
        writeln!(
            out,
            "prepared,{trial},{prepared},{}",
            context.prepared_glyphs()
        )
        .map_err(|e| e.to_string())?;
        black_box(&frame);
    }
    Ok(())
}

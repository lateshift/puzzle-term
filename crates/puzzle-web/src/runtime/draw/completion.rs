//! Completion work shares one measured refresh with ordinary browser frame work.
//!
//! A p90 history uses the same 32-sample horizon and one-millisecond scheduling
//! margin as native frame pacing (§10.4). Publication is removed from each cost
//! sample, avoiding feedback that shrinks its own allowance. No glyph count or
//! benchmark-specific deadline decides how much ready work reaches an atlas.
//! Publication and predictable source work consume the same refresh allowance.
//! The observer records actual costs without changing the host's admission decision.
//! Submission timing comes from the retained frame rather than callback count.
//! Only active development workloads retain the per-turn observation vector.
//! Covered-glyph readiness follows actual atlas handles in the submitted rows.
//! Unsupported diagnostics cannot be misreported as delayed covered ink.
//! An ordinary-cost sample excludes both publication and inline source preparation.
use super::Runtime;
use std::time::Duration;

/// Existing frame-pacing observation horizon, independent of completion throughput.
const SAMPLES: usize = 32;

/// Leaves measured ordinary CPU and scheduling slack inside the observed rAF period.
pub(super) fn budget(runtime: &Runtime) -> Duration {
    let mut samples = [Duration::ZERO; SAMPLES];
    let count = runtime.ordinary_costs.len();
    for (slot, cost) in samples.iter_mut().zip(&runtime.ordinary_costs) {
        *slot = *cost;
    }
    samples[..count].sort_unstable();
    let cost = samples[..count]
        .get((count * 9).div_ceil(10).saturating_sub(1))
        .copied()
        .unwrap_or_default();
    // rAF's tick is the start of the available refresh; callback setup and resize
    // have already consumed part of it. Keep that elapsed time in the allowance.
    let left = runtime.tick.map_or(Duration::ZERO, |at| {
        (at + runtime.period).saturating_duration_since(web_time::Instant::now())
    });
    if let (Some(tick), Some(text)) = (runtime.tick, runtime.text.as_ref()) {
        // Sources spend wall-clock slack between paints, so adjacent tasks cannot
        // each invent a fresh whole refresh allowance or wait for idle priority.
        for stats in text.fallback_stats.borrow().iter() {
            stats.refresh(tick, runtime.period, cost);
        }
    }
    (runtime.period / 2).min(left.saturating_sub(cost + Duration::from_millis(1)))
}

/// Successful frames supply CPU samples; long idle time never becomes a cost sample.
pub(super) fn record(runtime: &mut Runtime, cost: Duration) {
    if runtime.ordinary_costs.len() == SAMPLES {
        runtime.ordinary_costs.pop_front();
    }
    runtime.ordinary_costs.push_back(cost);
}

/// Retains actual submitted-frame costs and ink, never requesting replacement geometry.
#[cfg(any(test, feature = "measurements"))]
pub(super) fn observe(
    runtime: &mut Runtime,
    context: &mut puzzle_term::frame_builder::context::TextContext,
    started: web_time::Instant,
    allowance: Duration,
) {
    if runtime.measurements.active {
        // Observe only the real submitted handles; this does not request another draw.
        let end = runtime.started.elapsed();
        let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
        let start = started.saturating_duration_since(runtime.started);
        let deadline = runtime.tick.unwrap_or(started) + runtime.period;
        runtime.measurements.turns.push([
            ms(start),
            ms(allowance),
            ms(started.elapsed()),
            ms(context.source_preparation_time()),
            ms(context.completion_time()),
            ms(deadline.saturating_duration_since(runtime.started)),
            ms(end),
        ]);
        // The host builder retains the handles of the actual submitted image.
        // Querying Session here would mix parsed rows with an earlier presentation.
        let cells = runtime
            .render
            .builder
            .as_ref()
            .map_or_else(Vec::new, |builder| builder.retained_ink_samples(context));
        runtime.measurements.ink.presented(end, cells);
    }
}

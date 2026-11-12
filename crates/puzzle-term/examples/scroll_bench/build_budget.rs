//! Exact native row-build scope, after the separately budgeted Session snapshot.
//!
//! The original M8 probe still times its complete convenience call. This extra
//! regression follows LiveWindow: boundary maintenance and snapshot capture,
//! then a timed build_snapshot from the sealed publication. Each trial exposes one
//! untouched history row with warm glyphs. Both DPRs and all raw samples remain.
//! No median can conceal a 4-ms build here: every build must stay within 0.2 ms.
//! Formatting, parsing, warming and snapshot copying are outside this build timer.
//! A maximum failure is reported only after writing every sample, never discarded.
use super::*;
use puzzle_term::frame_snapshot::FrameSnapshot;

/// Measures the production build boundary at both scales and rejects any over-budget build.
pub(super) fn run() -> Result<(), String> {
    let mut csv = String::from("dpr,sample,top,shift,build_ms,rebuilt_rows\n");
    let worst = measure(1.0, &mut csv)?.max(measure(2.0, &mut csv)?);
    // Preserve failures before returning an error to the unchanged acceptance driver.
    std::fs::write(format!("{}/exposed-row-build.csv", evidence_dir()), csv)
        .map_err(|e| e.to_string())?;
    if !worst.is_finite() || worst > 0.2 {
        return Err(format!("exposed scroll row build {worst:.6} ms > 0.2"));
    }
    Ok(())
}

/// The first trial rebuilds exactly one history row, without prebuilding that row.
fn measure(dpr: f32, csv: &mut String) -> Result<f64, String> {
    let (mut context, mut builder, terminal, mut frame) = fixture::new(dpr)?;
    let mut snapshot = FrameSnapshot::default();
    // Warm only the late-copy pool against the live grid, as ordinary startup does.
    snapshot.capture(&terminal, builder.style_epoch())?;
    let prepared = context.prepared_glyphs();
    // Keep sample zero: it is the first newly exposed row, rather than a prewarmed cache hit.
    let mut worst: f64 = 0.0;
    for step in 0..101 {
        let top = terminal.screen().ring.scrollback_len() - step - 1;
        let shift = if step % 2 == 0 {
            -(context.metrics().height_px as i32 / 2)
        } else {
            0
        };
        // These operations belong to the pre-build boundary, matching the render thread.
        context.begin_frame()?;
        builder.sync_generation(context.atlas_generation());
        snapshot.capture_scrolled(&terminal, builder.style_epoch(), top, shift)?;
        let started = Instant::now();
        builder.build_snapshot(&mut context, &snapshot, &mut frame)?;
        let millis = started.elapsed().as_secs_f64() * 1000.0;
        black_box(frame.slugs.len());
        // Timing an unchanged cache hit or preparing new outlines would change the workload.
        if builder.rebuilt_rows() != 1 || context.prepared_glyphs() != prepared {
            return Err("build trial did not expose one row with warmed glyphs".into());
        }
        worst = worst.max(millis);
        // Recording retains even the worst sample without charging I/O to another frame.
        csv.push_str(&format!("{dpr},{step},{top},{shift},{millis:.6},1\n"));
        frame.uploads.clear();
    }
    Ok(worst)
}

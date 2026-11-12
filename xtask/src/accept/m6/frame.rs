//! Independent M6.3 frame, scheduling and CPU-budget acceptance lines.
//!
//! Each line names a behavior from docs/tasks/M6.md. Exact Cargo test
//! selection prevents a renamed test from reporting a false PASS. The
//! native release probe times only frame construction, and both budget
//! lines use one measured run so the two numbers describe the same host.
//!
//! The color lines each protect a different source of frame color:
//! retained palette tags, OSC changes and render-time contrast policy.
//! Combining them into one opaque case would let a policy regression hide
//! behind an unrelated palette success. The underline lines similarly
//! separate Slug wave preparation from the pixel-rect dash pattern.
//!
//! Cursor checks cover three independent state owners. DECSCUSR chooses
//! geometry, DECTCEM controls visibility, and the platform focus flag
//! replaces a filled block with an outline. A wide leading cell owns its
//! spacer, so the caret must cover two physical column widths there.
//!
//! The mode-2026 case uses a manual clock: a parsed but withheld frame
//! remains dirty until reset or the 200 ms safety deadline. The blink
//! case advances host time past 15 seconds and checks that the final phase
//! is visible. These tests guard the no-polling idle guarantee (P2).
//!
//! Row caching and allocation have separate lines. The first proves a
//! stable LineId/version/style epoch avoids reshaping. The second counts
//! heap requests while copying warm cached instances into repeated frames.
//! The golden scenario then exercises their GPU output with VT colors,
//! underlines, cursor forms, wide text and box drawing in one small target.
//!
//! The release CPU probe invalidates all cached row colors without
//! repacking the font atlas, which represents a warm full redraw. Its
//! one-row trial edits a VT row before timing, so the measured interval
//! contains only neutral frame construction. A median rejects occasional
//! scheduler interruptions without changing either §15.2 threshold.
use super::{report, session_test};

/// Runs one test and rejects Cargo's successful zero-selected case.
fn term_test(name: &str) -> Result<(), String> {
    session_test(name)
}

/// Runs the warmed native 240×70 example and parses its two medians.
pub(in crate::accept) fn cpu_budget() -> Result<(f64, f64), String> {
    let output = crate::commands::capture(
        "cargo",
        &[
            "run",
            "--release",
            "-p",
            "puzzle-term",
            "--example",
            "frame_bench",
        ],
    )?;
    let line = output
        .lines()
        .find(|line| line.starts_with("full_us="))
        .ok_or("frame benchmark did not print both timings")?;
    let mut values = line.split_whitespace();
    let full = parse_time(values.next(), "full_us=")?;
    let row = parse_time(values.next(), "row_us=")?;
    if !full.is_finite() || !row.is_finite() {
        return Err("nonfinite frame timing".into());
    }
    Ok((full, row))
}

/// Validates one labelled microsecond value from the release probe.
fn parse_time(value: Option<&str>, prefix: &str) -> Result<f64, String> {
    value
        .and_then(|text| text.strip_prefix(prefix))
        .ok_or_else(|| format!("missing {prefix} timing"))?
        .parse()
        .map_err(|error| format!("invalid {prefix} timing: {error}"))
}

/// Converts one measured median to a budget result without relaxing it.
pub(in crate::accept) fn within(value: f64, limit: f64, label: &str) -> Result<(), String> {
    if value <= limit {
        Ok(())
    } else {
        Err(format!("{label} {value:.3} µs exceeds {limit:.0} µs"))
    }
}

/// Reports every currently implemented M6.3 criterion separately.
pub(super) fn report_frame(passed: &mut bool) {
    // Distinct lines show whether color resolution, geometry or scheduling
    // broke. A later success cannot hide an earlier failed behavior (R14).
    for (criterion, test) in [
        // Palette and inverse ordering must be independent of the VT's
        // retained color tags; OSC changes recolor existing cells.
        (
            "M6.3 indexed/inverse/dim colors",
            "frame_builder::colors::tests::indexed_and_inverse_color_rules",
        ),
        (
            "M6.3 minimum contrast",
            "frame_builder::colors::tests::minimum_contrast_meets_requested_ratio",
        ),
        (
            "M6.3 OSC color overrides",
            "frame_builder::colors::tests::osc_overrides_reach_resolver_and_clear",
        ),
        // The wave is a Slug outline while dotted and dashed lines use
        // pixel-aligned rects; these paths can regress separately.
        (
            "M6.3 curly SGR 58 underline",
            "frame_builder::tests::curly_underline_tiles_and_uses_underline_color",
        ),
        (
            "M6.3 dotted/dashed underline",
            "frame_builder::tests::patterned_underlines_have_gaps",
        ),
        // DECSCUSR, DECTCEM, focus and two-cell spans alter only the
        // cursor layer, so no row content should need reparsing.
        (
            "M6.3 cursor styles and visibility",
            "frame_builder::tests::cursor_styles_follow_terminal_state",
        ),
        (
            "M6.3 wide cursor and focus",
            "frame_builder::tests::cell_ownership::wide_cursor_and_unfocused_outline",
        ),
        // The two timers use host time, leaving puzzle-vt deterministic.
        (
            "M6.3 mode-2026 safety timeout",
            "session::tests::synchronized_output_waits_then_times_out",
        ),
        (
            "M6.3 idle blink cutoff",
            "session::tests::blink::blinking_stops_without_input",
        ),
        // The builder must skip unchanged shaping and reuse allocation
        // capacity when it copies retained row instances into a new frame.
        (
            "M6.3 unchanged row cache",
            "frame_builder::tests::unchanged_row_reuses_its_geometry",
        ),
        (
            "M6.3 zero warm-frame allocations",
            "frame_builder::tests::warm_frame_assembly_allocates_nothing",
        ),
    ] {
        report(criterion, term_test(test), passed);
    }
    report(
        "M6.3 colors, underlines, cursor, wide and box golden",
        crate::golden::run_terminal(false, false),
        passed,
    );
    report_cpu(passed);
}

/// Reports both unchanged §15.2 limits from one release measurement.
fn report_cpu(passed: &mut bool) {
    // One release run produces both medians. A parse error is reported on
    // both lines so no part of the native CPU budget disappears from R14.
    let measured = cpu_budget();
    report(
        "M6.3 full 240x70 frame CPU <= 2 ms",
        measured
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|(full, _)| within(*full, 2_000.0, "full frame")),
        passed,
    );
    report(
        "M6.3 one-row frame CPU <= 0.2 ms",
        measured
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|(_, row)| within(*row, 200.0, "one row")),
        passed,
    );
}

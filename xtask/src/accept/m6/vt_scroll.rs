//! Tmux region-scroll regression gates for M6 (§6.3, P4).
//!
//! A one-line scroll above tmux's status bar must keep that bar's row
//! cache and must not scan every retained scrollback line. The benchmark
//! uses the actual VT parser with 10,000 history rows, comparing SU with
//! a full-screen LF on the same grid. Construction is outside the timer.
//! The speed criterion is a ratio on one machine rather than an absolute
//! nanosecond target: CPU frequency and timer overhead vary by host, while
//! the regression is SU becoming proportional to retained history.
//! The damage criterion is separate because a fast SU can still rebuild
//! tmux's unchanged status row in every rendered frame.
use super::{report, vt_test};
use std::fs;

/// Reads the ratio written by the release probe after running it afresh.
fn scroll_ratio() -> Result<(), String> {
    // A fresh release run replaces the prior artifact. Reading a stale
    // report would let a broken working tree inherit an earlier PASS.
    crate::commands::capture(
        "cargo",
        &[
            "run",
            "--release",
            "-p",
            "puzzle-vt",
            "--example",
            "scroll_region",
        ],
    )?;
    // The probe records raw times too, so a reviewer can inspect the
    // denominator and see whether the ratio hid a no-op baseline.
    let report = fs::read_to_string("target/corpus/scroll-region-ns.txt")
        .map_err(|error| format!("missing scroll-region probe output: {error}"))?;
    // A missing or malformed ratio means the probe did not complete its
    // contract; accepting an empty file would weaken this budget check.
    let ratio = report
        .lines()
        .find_map(|line| line.strip_prefix("ratio="))
        .ok_or("scroll-region probe omitted its ratio")?
        .parse::<f64>()
        .map_err(|error| format!("invalid scroll ratio: {error}"))?;
    // The reviewer set the 2× bound for SU 1 in a 1;47 region with full
    // scrollback. It is an acceptance threshold, not a tuning parameter.
    if ratio <= 2.0 {
        Ok(())
    } else {
        Err(format!("region SU is {ratio:.3}× full-screen LF, above 2×"))
    }
}

/// Reports damage locality and the independent native speed criterion.
pub(super) fn report_scroll(passed: &mut bool) {
    // These exact test names prevent Cargo's zero-match success from
    // claiming a row-cache guarantee after a renamed or removed test.
    report(
        "M6 tmux region scroll preserves rows outside margins",
        vt_test("terminal::tests::region_scroll_damage_excludes_footer").and_then(|()| {
            vt_test("terminal::tests::rectangular_scroll_damage_excludes_outer_rows")
        }),
        passed,
    );
    // Report the benchmark on its own line so a speed failure cannot be
    // masked by correct pixels or by a passing damage-bit test.
    report(
        "M6 tmux region SU <= 2x full-screen LF with 10000 history rows",
        scroll_ratio(),
        passed,
    );
}

//! Release timing gates for changed terminal rows (design §15, P1, P4).
//!
//! The four timers isolate different lifetimes. An exact run hit borrows
//! shaped IDs; a direct ASCII miss fills the cmap result; a Harfrust miss
//! runs substitutions; a warm full row also resolves graphemes, fallback
//! and grid placement. Reporting each separately shows where a regression
//! entered the frame budget. All timings come from one release process.
//!
//! The allocation assertion is separate from timing. A row can be fast
//! while still making transient heap requests that harm frame pacing.
//!
//! A 240-cell row is the wide-screen case used by the frame budget. The
//! 1 µs run hit protects the cheap lookup itself; the 5 µs ASCII path
//! protects ordinary text without GSUB; the 100 µs Harfrust allowance
//! covers rarer newly seen ligature runs. The 10 µs full-row limit includes
//! face selection and placement, so it cannot be satisfied merely by
//! bypassing shaping. The allocation test alternates two warmed rows,
//! detecting a fresh buffer request on each changed-row transition.
use super::{report, text_test};

/// Runs the native release probe used by every CPU budget line.
fn measured_budgets() -> Result<String, String> {
    // Debug code has a different cost model from the native frame budget.
    crate::commands::capture(
        "cargo",
        &["run", "--release", "-p", "xtask", "--", "m4-bench"],
    )
}

/// Rejects missing, invalid or over-limit measurements by their own name.
fn metric_budget(output: &str, name: &str, maximum_us: f64) -> Result<(), String> {
    let prefix = format!("M4_METRIC {name}=");
    let value = output
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .ok_or_else(|| format!("missing {name} measurement"))?;
    let measured: f64 = value
        .parse()
        .map_err(|_| format!("invalid {name} measurement"))?;
    // NaN would evade an ordinary greater-than comparison, so it is
    // explicitly invalid rather than treated as a fast measurement.
    if !measured.is_finite() || measured < 0.0 || measured > maximum_us {
        return Err(format!(
            "{name} measured {measured:.3} us, budget {maximum_us:.3} us"
        ));
    }
    Ok(())
}

/// Reports every user-approved M4 timing and P1 allocation criterion.
pub(super) fn report_shape_budgets(passed: &mut bool) {
    // A slow path fails its own criterion while the other medians remain
    // visible to the reviewer, as the R14 acceptance protocol requires.
    let budgets = measured_budgets();
    report(
        "pack and hinted ASCII first-frame CPU budgets",
        budgets.as_ref().map(|_| ()).map_err(Clone::clone),
        passed,
    );
    for (criterion, key, limit) in [
        ("240-cell run-cache hit <= 1 us", "cache_hit_us", 1.0),
        (
            "240-cell uncached ASCII fast path <= 5 us",
            "ascii_miss_us",
            5.0,
        ),
        (
            "240-cell uncached Harfrust ligature row <= 100 us",
            "harfrust_miss_us",
            100.0,
        ),
        (
            "240-cell warm full row layout <= 10 us",
            "row_layout_us",
            10.0,
        ),
    ] {
        report(
            criterion,
            budgets
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|output| metric_budget(output, key, limit)),
            passed,
        );
    }
    // This test uses allocation_counter around repeated warm row changes.
    report(
        "steady-state row layout allocates zero times",
        text_test("layout::tests::warm_row_layout_allocates_nothing"),
        passed,
    );
}

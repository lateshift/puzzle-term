//! D-47 acceptance keeps raster correctness, warm budgets and measured cost distinct.
//!
//! Existing M16.2 windows exercise the default GPU adapter for all real workloads
//! and reusable-glyph floods. These additional witnesses compare pixels against
//! vectors, force several page reuses within one frame, and destroy an actual GPU.
//! None substitutes a settling image for the first covered-glyph presentation.
//! The maximized Unicode run retains every build and its cache warm-up curve.
//! Raster cost is recorded separately from outline-to-bands preparation, following
//! the user's 2026-10-07 clarification; it has no invented fixed ceiling.
use super::super::{m7::test, report};
/// CPU/GPU raster area and curve-normalized samples have no new numerical ceiling.
mod cost;
/// The warmed bitmap path must retain the exact exposed-history-row CPU budget.
mod scroll;
/// Complete native pressure at maximized size, retaining chronological cache counts.
mod stress;

/// Every independent D-47 witness reports even if another criterion failed (R14).
pub(super) fn run(passed: &mut bool) {
    for (label, name) in [
        (
            "M16.4 DPR 1/2 bitmap/vector pixels and zero warm allocations",
            "grid_rasterizer::tests::grid_bitmap_matches_vectors_at_both_dprs_and_restores",
        ),
        (
            "M16.4 GPU-only page reuse preserves exact earlier/later pixels",
            "grid_rasterizer::tests::gpu_grid_flushes_preserve_exact_pixels_before_and_after_reuse",
        ),
        (
            "M16.4 real device loss rebuilds masks and restores image-only emoji",
            "grid_rasterizer::tests::recovery::real_device_loss_rebuilds_grid_masks_and_restores_authored_emoji",
        ),
    ] {
        // An exact test name cannot accidentally pass from an empty cargo filter.
        report(label, test("puzzle-embed", name), passed);
    }
    // Legacy vector-overflow masks have outline sources and must not acquire image-only mirrors.
    report(
        "M16.4 CPU vector overflow is mirror-free and authored neighbors restore",
        test(
            "puzzle-text",
            "raster::tests::outline_overflow_has_no_mirror_and_loss_preserves_authored_neighbors",
        ),
        passed,
    );
    // Allocation and exact build timing use the same newly exposed history-row workload.
    report(
        "M16.4 bitmap exposed row ≤0.2 ms and zero warm allocations",
        scroll::run(),
        passed,
    );
    // D-45 measures complete Unicode frames without applying the reusable-flood floor.
    report(
        "M16.4 maximized Unicode steady warm cache and complete frames",
        stress::run(),
        passed,
    );
    // GPU sampling includes atlas copies, without any CPU mirror or readback.
    report(
        "M16.4 CPU/GPU raster cost per glyph/curve/device pixel recorded",
        cost::run(),
        passed,
    );
}

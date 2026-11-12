//! Glyph budgets are exercised through actual cache, frame and GPU owners.
//! Each exact test must execute; Cargo's empty filter success is not evidence.
//! Pressure witnesses compare pixels across several same-frame page reuses.
use super::report;
/// Exact witnesses cover each independently observable part of the policy.
const CASES: [(&str, &str, &str); 8] = [
    // The accepted floor follows physical page sizes, so a future format change
    // must update validation without a separate hand-maintained MiB literal.
    (
        "glyph budget derives and validates complete-frame minimum",
        "puzzle-term",
        "frame_builder::context::tests::budget::structural_minimum_is_validated_and_reported_every_time",
    ),
    // Repeated invalid file reloads must still report every clamp to callers
    // while the warning sink emits just one diagnostic per settings owner.
    (
        "glyph budget clamp logs once across reload",
        "puzzle-term",
        "settings::store::budget::tests::structural_clamp_logs_once_across_reload",
    ),
    // Two builders retain pack-only glyph addresses across shrink and growth.
    // Neither an unchanged VT nor missing full font tables may lose its ink.
    (
        "glyph budget defaults and shared-frame runtime reload",
        "puzzle-term",
        "frame_builder::context::tests::budget::runtime_reload_preserves_complete_shared_frames_and_defaults",
    ),
    // A smaller allowance must start page eviction sooner than the default.
    // Stamps decide the victim; the budget does not change the recency rule.
    (
        "glyph budget LRU starts at configured page capacity",
        "puzzle-text",
        "raster::budget::tests::nondefault_budget_evicts_at_configured_capacity",
    ),
    // Native generated masks and web authored pixels retain their old shares
    // at 48 MiB, while the structural floor admits one page of each kind.
    (
        "glyph budget keeps native and web default page shares",
        "puzzle-text",
        "raster::budget::tests::default_format_capacities_and_minimum_fit",
    ),
    // A three-layer capacity must not round up to four sampled mask pages.
    // Shrinking also releases textures instead of changing only CPU bookkeeping.
    (
        "glyph budget GPU allocation and shrink obey configured bytes",
        "puzzle-embed",
        "grid_rasterizer::tests::budget::gpu_capacity_respects_non_power_of_two_budget_and_reload",
    ),
    // GPU-produced masks have no CPU mirror to conceal a missing upload.
    // Compare exact output on both sides of repeated same-frame page reuse.
    (
        "glyph budget generated masks preserve pixels through in-frame reuse",
        "puzzle-embed",
        "grid_rasterizer::tests::gpu_grid_flushes_preserve_exact_pixels_before_and_after_reuse",
    ),
    // Mixed mask/color pressure exercises the ordered flush boundary itself.
    // Earlier draws must keep their pixels after the page acquires new contents.
    (
        "glyph budget mixed frames stay complete through repeated flushes",
        "puzzle-desktop",
        "slug_probe::pressure_tests::raster::within_frame_flushes_preserve_exact_pixels_and_final_residency",
    ),
];

/// Runs ordered CPU policy and GPU completeness/reload contracts.
pub(super) fn run(passed: &mut bool) {
    for (criterion, package, test) in CASES {
        // Each failure is reported independently; later criteria still execute.
        report(criterion, super::super::m7::test(package, test), passed);
    }
}

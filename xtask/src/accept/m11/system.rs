//! Native discovery checks preserve deterministic harnesses and desktop boundaries.
//!
//! Tests execute actual forced-empty and configured-family contracts. Installed
//! coverage additionally rejects missing-glyph fonts before accepting a family.
//! Real Metal frames use explicit opt-in sources; normal goldens stay bundled.
//! Benchmarks retain the §15 outline-to-texels threshold and execute in release.
//! Rust heap counters are reported separately from virtual mapped font bytes.
//! Coverage tests shape the complete cluster and require owned visible ink;
//! a cmap entry alone would accept macOS LastResort missing-glyph boxes.
//! The empty discovery test follows the chain through final tofu frame emission.
//! Family order uses two locally registered aliases of approved bundled bytes,
//! so an installed machine's naming order cannot change that contract.
//! Its warmed coverage measurement guards the allocation-free row path.
//! Memory evidence counts Rust-owned discovery allocations and reports virtual
//! mappings separately; it does not claim mmap bytes are resident heap.
//! The source-replacement regression reuses a numeric glyph identity with new
//! pixels, catching stale handles that a simple atlas generation check misses.
//! Native readback tests both scales and exact CPU-mirror restoration while
//! the measured preparation count remains unchanged across resource replacement.
//! The release probe calibrates every drawable primary glyph, then compares
//! CJK/Hangul quadratic curve counts and costs against that observed spread.
//! Every failure is retained while later criteria continue, following R14.
use super::super::{m7::test, report};
/// Adds the implemented desktop discovery criteria in work-order order.
pub(super) fn run(passed: &mut bool) {
    contracts(passed);
    // Replacing a source must drop old prepared ink even when glyph IDs are reused.
    report(
        "M11.6 source replacement invalidates prepared stores",
        test(
            "puzzle-term",
            "frame_builder::context::tests::source_replacement_invalidates_prepared_ink",
        ),
        passed,
    );
    report(
        "M11.6 native DPR 1/2 cells and CPU restoration",
        crate::commands::capture(
            "cargo",
            &["run", "-p", "puzzle-desktop", "--", "system-font-probe"],
        )
        .map(|output| print!("{output}")),
        passed,
    );
    // Discovery/parsing are excluded; actual fallback paths enter normal Slug preparation.
    report(
        "M11.6 primary median ≤20 µs and fallback cost per curve within primary spread",
        crate::commands::capture(
            "cargo",
            &[
                "run",
                "--release",
                "-p",
                "puzzle-desktop",
                "--",
                "system-font-bench",
            ],
        )
        .map(|output| print!("{output}")),
        passed,
    );
    println!(
        "REVIEW M11.7 desktop: user confirmed 2026-10-04; prompt, emoji sequences, CJK/symbols and tmux/nvim; README.md / README.md#m11; target/m11-system/specimen-{{1,2}}x.png"
    );
}

/// Runs each discovery contract through an exact nonempty test filter.
fn contracts(passed: &mut bool) {
    // These filters must each execute at least one test rather than silently skip.
    for (label, name) in [
        (
            "M11.6 installed whole-cluster CJK, emoji and rare-symbol resolution",
            "system_fonts::tests::installed_complete_cluster_resolution",
        ),
        (
            "M11.6 forced-empty system selects builtin tofu",
            "system_fonts::tests::empty_system_has_only_tofu",
        ),
        (
            "M11.6 configured family order and allocation-free coverage hits",
            "system_fonts::tests::configured_family_order",
        ),
        (
            "M11.6 mapped fonts released and Rust heap measured",
            "system_fonts::tests::native_mapping_memory_is_released",
        ),
    ] {
        report(label, test("puzzle-embed", name), passed);
    }
}

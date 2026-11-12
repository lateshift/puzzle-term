//! G3 is a copy of actual presentation, never an offscreen render of current state.
//!
//! Pure pixel checks protect crop/channel/alpha rules; the native ownership test
//! proves ready completions cannot replace a presented front. Both must run.
//! The C line exercises each native target through the public header and times
//! the optimized copy only. Debug setup/compilation is outside that interval.
//! No existing input-latency cohort or transferred M12.refresh observation runs.
//! A missing test name is a failure, not Cargo's successful empty-filter result.
//! The C program independently asserts capacity, colour and last-presented rules.
use crate::accept::report;

/// G3 copies completed native pixels; it never measures or reschedules key delivery.
pub(super) fn run() -> bool {
    // Preserve independent results: a pixel failure must not hide the C contract.
    let mut passed = true;
    report(
        "M12.tabs snapshot crop, channels and immutable native ownership",
        snapshot_tests(),
        &mut passed,
    );
    report(
        "M12.tabs portable C last-presented snapshot and 5K-to-360px <=4ms",
        crate::ffi::snapshot(),
        &mut passed,
    );
    passed
}
/// Require real test witnesses; an accidentally empty Cargo filter cannot pass.
fn snapshot_tests() -> Result<(), String> {
    // Transparent, asymmetric source pixels distinguish premultiplication,
    // row orientation and RGBA swizzling from a uniform background-only check.
    let output = crate::commands::capture(
        "cargo",
        &["test", "-p", "puzzle-sys", "snapshot_pixels_keep_crop"],
    )?;
    if !output.contains("::snapshot_pixels_keep_crop_rgba_and_premultiplied_edges ... ok") {
        return Err("missing snapshot pixel regression".into());
    }
    // Real imported storage verifies CPU access after the same-queue fence.
    // Run under the existing native GPU fixture to avoid competing devices.
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "-p",
            "puzzle-sys",
            "imported_images_complete_in_order",
        ],
    )?;
    if !output.contains(
        "::imported_images_complete_in_order_preserve_colour_and_bound_pool_ownership ... ok",
    ) {
        return Err("missing immutable native snapshot regression".into());
    }
    Ok(())
}

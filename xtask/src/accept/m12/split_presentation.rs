//! Real C/native-kind-5 first presentation at fractional pane dimensions.
//!
//! A parsed Session or a successful resize return cannot prove presentation.
//! The C fixture requires exact child bounds and IOSurface pixels, plus the
//! tick-delivered firstFrame event, for odd device sizes at both tested scales.
//! Its normal guard detects a hang; no latency budget or retry is added.

/// Run the native contract once, including its retained-layer resize cases.
pub(super) fn run() -> bool {
    let mut passed = true;
    // The same fixture remains in the full layer gate; this selector avoids
    // unrelated performance cohorts when reproducing a presentation defect.
    crate::accept::report(
        "M12 split physical geometry and first presentation",
        crate::ffi::host(),
        &mut passed,
    );
    passed
}

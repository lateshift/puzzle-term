//! M15.7 uses operation counts so throughput cannot regress behind noisy timings.
//! Former image/search capacity must not add work to an image-free terminal.
//! These tests observe the actual eviction loop and placeholder branch, rather
//! than timing a synthetic scroll and inferring that nothing extra happened.
//! Existing M16.8 witnesses keep the zero-cell and bounded-handle requirements.
use crate::accept::{m7, report};

/// Each structural invariant remains an independently failing acceptance line.
pub(super) fn run(passed: &mut bool) {
    // Retiring placement anchors cannot make subsequent scrolling proportional
    // to the largest image grid ever displayed in the terminal's lifetime.
    report(
        "M15.7 image-free scroll matches fresh row/handle/anchor counts after retirement",
        m7::test(
            "puzzle-vt",
            "tracked::tests::retired_anchors_add_zero_work_to_image_free_scrolls",
        ),
        passed,
    );
    // Removing dead entries must preserve generation checks and both screen domains.
    report(
        "M15.7 live-anchor eviction/reflow preserves handles with zero allocation",
        m7::test(
            "puzzle-vt",
            "tracked::tests::live_anchor_eviction_and_reflow_preserve_handles_without_allocating",
        ),
        passed,
    );
    // The conservative VT summary selects a different cell walker before the loop;
    // ordinary rows do not inspect scalar/cluster text for kitty metadata at all.
    report(
        "M15.7 ordinary ASCII/Unicode frames perform zero placeholder checks",
        m7::test(
            "puzzle-term",
            "frame_builder::tests::ink_dispatch::ordinary_rows_perform_zero_placeholder_checks",
        ),
        passed,
    );
}

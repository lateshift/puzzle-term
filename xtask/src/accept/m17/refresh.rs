//! Host-source policy is independent of native display/compositor measurements.
//!
//! Injected clocks prove settling and one reservation per physical refresh.
//! The actor witness checks sticky source choice, mailbox coalescing and close.
//! The independent C caller then drives the same public ABI; ASan/Miri remain
//! part of the existing FFI receipt rather than a second pointer implementation.
use crate::accept::{m7, report};

/// A task receipt never substitutes clock tests for the five hardware runs.
pub(super) fn api(passed: &mut bool) {
    for (criterion, filter) in [
        // A reply available before this tick's build deadline belongs to this tick,
        // rather than unconditionally adding one refresh of content latency.
        (
            "M17.refresh injected clocks, settling, same-refresh and duplicates",
            "window::pacing::link::tests::same_refresh_reply_has_its_own_build_deadline",
        ),
        // Native and host clocks must use the same platform-neutral burst rule.
        (
            "M17.refresh settle window and idle",
            "window::pacing::link::tests::link_tick_settles_at_snapshot_time_and_idle_has_no_deadline",
        ),
        // A blocking drawable does not teach or replace an externally supplied phase.
        (
            "M17.refresh FIFO cannot replace the supplied clock",
            "window::pacing::link::tests::link_clock_keeps_every_supplied_refresh_despite_fifo_wait",
        ),
        // Taking over an active surface cannot submit its last output twice.
        // Timer calibration would also create unsolicited frames on a host clock.
        (
            "M17.refresh takeover duplicate barrier and timer exclusion",
            "window::pacing::link::tests::host_takeover_preserves_output_barrier_and_disables_timer_calibration",
        ),
        // Reordered callbacks share one mailbox and cannot revive a closed driver.
        (
            "M17.refresh host source lifecycle and coalescing",
            "render_thread::tests::refresh::host_refresh_is_sticky_and_coalesces_without_touching_closed_driver",
        ),
        // A host crossing can arrive after this render turn sealed its observations.
        // The acquisition boundary must see that later fact before entering FIFO.
        (
            "M17.refresh crossing during a build cancels old-display acquisition",
            "render_thread::tests::refresh::display_crossing_during_build_cancels_old_acquisition",
        ),
        // D-54 still permits an old-size frame clipped at the top left on one display.
        // Adopting the destination observations must reopen acquisition normally.
        (
            "M17.refresh size-only clipping and target-display recovery",
            "render_thread::host::tests::acquisition_gate_preserves_size_only_frames_and_returns_to_target_display",
        ),
    ] {
        // m7::test rejects empty filters as well as failing test subprocesses.
        report(criterion, m7::test("puzzle-embed", filter), passed);
    }
    // An independent C translation unit calls the export and checks misuse errors;
    // Rust-only tests cannot detect a mismatched public declaration or ABI layout.
    report(
        "M17.refresh native C host callbacks and lifecycle",
        crate::ffi::host(),
        passed,
    );
}

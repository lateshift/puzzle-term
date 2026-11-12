//! D-53 lifetime acceptance for the retained per-display native refresh source.
//!
//! Scalar tests force detached and overlapping callbacks deterministically.
//! The macOS witness also receives real OS refreshes across repeated stop/start,
//! sibling-window detach and shutdown. None of these tests certify cadence;
//! the display-specific optimized cohorts supply that independent evidence.
//! Source clocks and snapshot receipt are separate contracts: an accurate native
//! tick is insufficient if the renderer still selects the preceding request.
//! The clock witnesses reserve the same refresh's receive opportunity and reject
//! repeated output identities after reactivation. Settling checks snapshot time,
//! rather than the future scanout timestamp, so transient editor ink stays hidden.
//! Lifetime tests deliberately permit callbacks after detach; they prove that
//! these callbacks retain valid storage while losing all surface/wake authority.
use super::super::{m7::test, report};

/// Requires every named lifetime/allocation witness to execute, never an empty filter.
pub(super) fn run(passed: &mut bool) {
    clocks(passed);
    for (label, name) in [
        (
            // Coalescing must not report a later callback as the restart's first.
            "restart observation preserves first callback",
            "first_callback_survives_coalescing_and_resets_on_restart",
        ),
        (
            // A post-stop straggler must remain memory-safe and suppress its wake.
            "shutdown during refresh flood",
            "close_and_shutdown_during_refresh_flood",
        ),
        (
            // A new subscription may not consume its predecessor's sealed clock.
            "move and disappearing display invalidate ticks",
            "moving_and_disappearing_displays_reject_obsolete_refreshes",
        ),
        (
            // This includes the actor's real bounded-channel notification path.
            "refresh publication/wake allocates nothing",
            "refreshed_mailbox_is_allocation_free_and_coalesces",
        ),
        (
            // Native stop does not join: two generations can overlap in flight.
            "overlapping callbacks preserve clock tuple",
            "overlapping_native_producers_publish_only_complete_clock_facts",
        ),
    ] {
        // Exact output matching protects the gate against removed/renamed witnesses.
        report(
            &format!("M16.7b {label}"),
            test("puzzle-sys", &format!("display_link::tests::{name}")),
            passed,
        );
    }
    // Only macOS owns a native link. Other platforms explicitly prove that their
    // unchanged timer path cannot accidentally enter foreign refresh glue.
    let name = if std::env::consts::OS == "macos" {
        "native_link_close_under_flood_and_process_shutdown"
    } else {
        "other_platforms_report_unsupported"
    };
    report(
        "M16.7b native link teardown or unsupported platform contract",
        test("puzzle-sys", &format!("display_link::tests::{name}")),
        passed,
    );
}

/// Injected source clocks distinguish physical refresh pacing from timer prediction.
fn clocks(passed: &mut bool) {
    for (label, name) in [
        // The deadline must retain room for the answer to this frame's own request.
        (
            "same-refresh owner answer fits build deadline",
            "same_refresh_reply_has_its_own_build_deadline",
        ),
        // A future scanout cannot expose incomplete editor updates before settling.
        (
            "link settling and idle without predicted deadlines",
            "link_tick_settles_at_snapshot_time_and_idle_has_no_deadline",
        ),
        // Every supplied 60-Hz tick stays usable despite FIFO waits; no phase learning.
        (
            "physical refresh sequence survives FIFO wait",
            "link_clock_keeps_every_supplied_refresh_despite_fifo_wait",
        ),
    ] {
        report(
            &format!("M16.7b {label}"),
            test(
                "puzzle-embed",
                &format!("window::pacing::link::tests::{name}"),
            ),
            passed,
        );
    }
}

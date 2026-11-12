//! M8.5 pointer and clipboard contracts before the desktop review STOP.
//!
//! Synthetic native samples pass through the actual desktop adapter and Session,
//! checking gestures as well as which bytes reach a tracking application.
//! Auto-scroll witnesses integrate a stationary outside position with injected
//! timestamps, and distinguish live-tail/history boundaries from active motion.
//! Clipboard tests use a recording service, avoiding the user's real clipboard.
//! Styled selection and whole-buffer HTML must resolve colors identically.
//! The complete Chrome suite separately reads both real ClipboardItem MIME blobs.

use super::{report, test};

/// Executes exact witnesses; missing or renamed tests cannot silently pass.
pub(super) fn contracts(passed: &mut bool) {
    pointer_contracts(passed);
    drag_contracts(passed);
    clipboard_contracts(passed);
    representation_contracts(passed);
}

/// Native adapter and injected-clock gestures guard ownership and idle behavior.
fn pointer_contracts(passed: &mut bool) {
    // Each entry protects a separate user-visible requirement of §6.9 or §14.9.
    // Tracking and local selection must remain mutually exclusive routes.
    for (label, package, name) in [
        // Real click counts choose word/line expansion before drag updates.
        // Shift extends the original press; Alt creates constant physical columns.
        (
            "M8.5 native press/drag/double/triple/Alt/Shift table",
            "puzzle-desktop",
            "mouse::tests::selection::desktop_pointer_selection_table",
        ),
        // Four tracking modes send application reports unless Shift selects locally.
        // Disabling shiftSelects must restore reporting even with Shift held.
        (
            "M8.5 tracking/Shift/shiftSelects routing table",
            "puzzle-desktop",
            "mouse::tests::selection::desktop_tracking_selection_table",
        ),
        // Single clicks clear without manufacturing a one-cell selection.
        // Triple click includes every row of a soft-wrapped logical line.
        (
            "M8.5 ordinary click clears and triple selects wrapped line",
            "puzzle-desktop",
            "mouse::tests::selection::desktop_click_clear_and_wrapped_line",
        ),
        // Press-only application modes still end local drag ownership on Shift release.
        // Restoring Shift alone cannot resume that drag without a new press.
        (
            "M8.5 Shift release ends local drag in X10/normal tracking",
            "puzzle-desktop",
            "mouse::tests::selection::tracking_shift_release_ends_local_drag",
        ),
    ] {
        // Native fixtures exercise immediate buttons and coalesced winit motion.
        report(label, test(package, name), passed);
    }
}

/// Edge motion uses one injected refresh clock and ends without idle polling.
fn drag_contracts(passed: &mut bool) {
    // A stationary outside pointer needs frames even without new OS motion events.
    for (label, package, name) in [
        // Two outside distances produce distinct integrated viewport displacement.
        // Release uses the shared row snap and ends the refresh loop.
        (
            "M8.5 distance speed, stationary edge drag, release snap and idle",
            "puzzle-term",
            "session::tests::pointer::edge_speed_and_stationary_autoscroll_timeline",
        ),
        // A rewritten range and out-and-back screen transition cancel the drag.
        // A reached history boundary is inert even while the button remains down.
        (
            "M8.5 edge-drag invalidation and history boundaries",
            "puzzle-term",
            "session::tests::pointer::edge_drag_cancellation_and_boundaries",
        ),
        // Long selections hash borrowed cells and reuse their row witnesses.
        // Replacement after warming must not allocate extracted text.
        (
            "M8.5 warm selection drag zero allocations",
            "puzzle-term",
            "session::tests::pointer::warm_selection_drag_allocates_nothing",
        ),
        // Release contributes its final hit before the queued copy executes.
        // The fake service must contain exactly that final Unicode interval.
        (
            "M8.5 copy-on-select final release range",
            "puzzle-term",
            "session::tests::pointer::pointer_copy_on_select_release",
        ),
        // A tracking change owns future pointer reports and cancels old local speed.
        // Its fractional displacement still reaches an aligned, idle endpoint.
        (
            "M8.5 tracking claim cancels edge drag and settles fractional position",
            "puzzle-term",
            "session::tests::pointer::tracking_claim_cancels_edge_drag_and_snaps",
        ),
    ] {
        // These gestures use the same adapter path as the live winit callbacks.
        report(label, test(package, name), passed);
    }
}

/// Explicit copy preserves style while policy and plain alternatives remain intact.
fn clipboard_contracts(passed: &mut bool) {
    // Rich export witnesses read independently chosen colors and span counts.
    // They protect both serializers rather than comparing only their own output.
    for (label, package, name) in [
        // Fake clipboard checks styles and plain extraction from the same cells.
        // HTML tags must never become the plain alternative.
        (
            "M8.5 text/VT/HTML copy with plain alternative",
            "puzzle-term",
            "session::tests::copy::copy_formats_and_plain_alternative",
        ),
        // Without selection, Ctrl+C remains the application interrupt byte.
        // With selection, copy consumes it without sending another interrupt.
        (
            "M8.5 performable copy preserves interrupt without selection",
            "puzzle-term",
            "session::tests::copy::performable_copy_passes_interrupt_without_selection",
        ),
        // The existing clipboard kind controls both text and rich writes.
        // An unfinished command must not replace the most recent completed output.
        (
            "M8.5 write-policy veto and latest finished output",
            "puzzle-term",
            "session::tests::copy::rich_copy_policy_and_latest_finished_output",
        ),
        // Equal C/D boundaries are a valid empty command output.
        // Copying an earlier nonempty result would misidentify the last command.
        (
            "M8.5 empty completed output supersedes previous output",
            "puzzle-term",
            "session::tests::copy::copy_latest_empty_command_output",
        ),
    ] {
        // These copies use the real request/write policy chain with a fake service.
        report(label, test(package, name), passed);
    }
}

/// Markup resolution and external payloads guard the two clipboard representations.
fn representation_contracts(passed: &mut bool) {
    // Representation cases never read or modify the operating-system clipboard.
    for (label, package, name) in [
        // Underline and strike coexist in one decoration property.
        // Inverse then faint resolve before equal-style cells merge into spans.
        (
            "M8.5 shared HTML style, decorations, inverse/faint and merged spans",
            "puzzle-vt",
            "view::serialize::tests::html_exports_share_resolved_runs",
        ),
        // A soft wrap preserves the span while a hard newline closes it.
        // Hidden text remains selectable without visible glyph ink.
        (
            "M8.5 HTML soft continuity, hard boundaries and hidden text",
            "puzzle-vt",
            "view::serialize::tests::html_runs_join_soft_wrap_and_preserve_hidden",
        ),
        // All copy formats round-trip; rich writes require both strings.
        // Missing, mistyped or extra keys fail before any platform side effect.
        (
            "M8.5 Copy and rich ClipboardWrite wire validation",
            "puzzle-term",
            "messages::tests::selection::copy_and_rich_clipboard_wire_contracts",
        ),
        // Signed outside positions round-trip without clamping away distance.
        // NaN, infinite coordinates and invalid metrics are rejected at the wire.
        (
            "M8.5 Pointer signed coordinates and geometry wire validation",
            "puzzle-term",
            "messages::tests::selection::pointer_wire_rejects_invalid_geometry",
        ),
    ] {
        // Existing test helper rejects zero-test filters, preserving mechanical evidence.
        report(label, test(package, name), passed);
    }
}

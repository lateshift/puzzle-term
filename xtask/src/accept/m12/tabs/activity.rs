//! Activity is a Session-owned unread latch, shared by all native hosts.
//!
//! The owner test separates accepted bytes from empty continuation turns and
//! checks pane isolation. The C witness proves the additive exported call and
//! tick-only null callback using a real host transport. Neither gate measures
//! hardware latency or certifies the app's tab presentation (R15).
use crate::accept::report;

/// Keep owner and portable C observations independent in the task report.
pub(super) fn run() -> bool {
    let mut passed = true;
    report(
        "M12.tabs per-session activity acknowledgement",
        core(),
        &mut passed,
    );
    // The fixture invokes seen and feeds ordered output through puzzle.h; it
    // cannot pass solely because a private Session helper behaves correctly.
    report(
        "M12.tabs C activity callback and seen contract",
        crate::ffi::host(),
        &mut passed,
    );
    passed
}

/// Cargo's successful empty filter is not evidence for unread-event behavior.
fn core() -> Result<(), String> {
    let name = "activity_is_one_output_edge_per_host_acknowledgement";
    let output = crate::commands::capture("cargo", &["test", "-p", "puzzle-term", name])?;
    if !output.contains(&format!("::{name} ... ok")) {
        return Err("missing Session activity lifecycle regression".into());
    }
    // Any assertion failure already propagates through capture, preserving the
    // one-event, no-empty-turn and cross-session checks rather than sampling UI.
    Ok(())
}

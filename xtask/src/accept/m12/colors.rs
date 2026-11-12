//! D-61 native chrome colour facts use the Session outbox and checked C consumer.
//!
//! The owner test rejects duplicate events during ordinary/SGR output. The C
//! fixture separately observes creation, exact OSC bytes/reset and theme switches.
//! Neither test reads a Session through the host or invents a colour from its name.
//! Existing real-surface byte/profile gates remain part of the layer task; this
//! task adds event delivery, not another rendering colour space or pixel tolerance.
use crate::accept::report;

/// Task-local checks retain the independent native callback witness.
pub(super) fn run() -> bool {
    let mut passed = true;
    report(
        "M12.colors change-only default RGB owner events",
        core(),
        &mut passed,
    );
    report(
        "M12.colors C creation, OSC defaults/reset and appearance",
        crate::ffi::host(),
        &mut passed,
    );
    passed
}

/// Require the result name as well as Cargo success, rejecting an empty filter.
fn core() -> Result<(), String> {
    let output = crate::commands::capture(
        "cargo",
        &["test", "-p", "puzzle-term", "session::tests::host_colors"],
    )?;
    // A renamed test cannot silently certify duplicate suppression or OSC defaults.
    if !output.contains("::default_colors_emit_once_then_follow_osc_changes_and_resets ... ok") {
        return Err(format!("missing colour owner regression: {output}"));
    }
    Ok(())
}

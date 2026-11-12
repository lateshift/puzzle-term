//! Embedders receive semantic tab facts without parsing the terminal byte stream.
//! These task gates run functional contracts only, never a desktop timing round.
//! Real-shell fixtures pin the source command and preserve semantic boundaries;
//! the host-outbox fixture covers bounded metadata and companion tab facts.
//!
//! The command selector can be checked independently while activity delivery is
//! implemented in its own checkpoint. Neither selector certifies native tab UI;
//! that look and interaction review belongs to the host and the user (R15).
use crate::accept::report;

/// G1 spans parser, typed bus, host outbox and every supported snippet route.
pub(super) fn command() -> bool {
    let mut passed = true;
    report(
        "M12.tabs bounded command labels and shell source",
        command_checks(),
        &mut passed,
    );
    passed
}

/// A successful empty Cargo filter must not certify a missing contract test.
fn command_checks() -> Result<(), String> {
    let output = crate::commands::capture(
        "cargo",
        &["test", "-p", "puzzle-term", "session::tests::host_shell"],
    )?;
    // Labels and companion facts are independent observer subscriptions. Passing
    // the metadata decoder cannot certify cwd/progress/finished delivery by itself.
    for name in [
        "command_started_carries_bounded_labels_through_the_host_outbox",
        "shell_companion_facts_reach_the_host_outbox",
    ] {
        if !output.contains(&format!("::{name} ... ok")) {
            return Err(format!("missing host shell regression: {name}"));
        }
    }
    // Fish with native marks and its feature-disabled fallback are separate
    // routes; both must work without duplicate output-start boundaries.
    let output = crate::commands::capture(
        "cargo",
        &["test", "-p", "puzzle-embed", "snippet_real_pty_zones"],
    )?;
    for shell in ["bash", "zsh", "fish", "fish_legacy"] {
        if !output.contains(&format!("::{shell}_snippet_real_pty_zones ... ok")) {
            return Err(format!("missing {shell} command-label source regression"));
        }
    }
    Ok(())
}

/// Explicit unread acknowledgements have a separate core/C lifecycle gate.
mod activity;
/// Run the independent G2 checkpoint without repeating shell-source tests.
pub(super) fn activity() -> bool {
    activity::run()
}
/// Pixel and native C copy criteria are independent of G1/G2 owner behavior.
mod snapshot;
/// Run only the snapshot checkpoint; the held key-resize observation stays separate.
pub(super) fn snapshot() -> bool {
    snapshot::run()
}

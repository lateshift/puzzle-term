//! M6.5 settings acceptance against the neutral store and native adapter.
//!
//! The store owns typed precedence and validation without a file format.
//! The native adapter separately proves that serde_json persistence preserves
//! unknown keys and that malformed edits leave the running values intact.
use super::super::report;

/// Runs an exact test so Cargo's successful zero-match result cannot pass.
fn exact(package: &str, target: &str, name: &str) -> Result<(), String> {
    let output = crate::commands::capture(
        "cargo",
        &["test", "-p", package, target, name, "--", "--exact"],
    )?;
    if output.contains(&format!("test {name} ... ok")) {
        Ok(())
    } else {
        Err(format!("{name} did not run"))
    }
}

/// Reports one independent setting contract without hiding later failures.
fn check(passed: &mut bool, label: &str, package: &str, target: &str, name: &str) {
    report(label, exact(package, target, name), passed);
}

/// Adds every M6.5 work-order criterion to the milestone report.
pub(super) fn report_settings(passed: &mut bool) {
    // A session override must win without mutating the persisted user value.
    // The neutral test exercises all four layers, including a named profile.
    // A missing profile layer would otherwise look like a valid CLI override.
    check(
        passed,
        "M6.5 layer precedence",
        "puzzle-term",
        "--lib",
        "settings::store::tests::layers_resolve_in_declared_order",
    );
    // Validation is deliberately per key: one malformed sibling must not
    // discard another valid change in the same patch. A bounded number is
    // clamped, while a wrong type retains the previous effective value.
    check(
        passed,
        "M6.5 per-key clamping and rejection reports",
        "puzzle-term",
        "--lib",
        "settings::store::tests::clamping_and_rejection_are_per_key",
    );
    // A newer config may contain keys this binary does not understand.
    // Native save must keep them after JSON parsing and pretty printing,
    // rather than merely retaining them in the in-memory neutral tree.
    check(
        passed,
        "M6.5 unknown JSON keys survive a round trip",
        "puzzle-embed",
        "--lib",
        "settings::tests::unknown_values_round_trip",
    );
    // The adapter reads a named profile from the user document, then applies
    // CLI --set at session priority. The same test rejects malformed CLI
    // values before the window starts with a silently wrong font size.
    check(
        passed,
        "M6.5 profile activation and CLI precedence",
        "puzzle-embed",
        "--lib",
        "settings::tests::profile_and_set_precedence",
    );
    // Editors often write an incomplete JSON file during a live reload.
    // The active user and CLI layers must remain usable until a corrected
    // document arrives, so a failed parse cannot reset the terminal.
    check(
        passed,
        "M6.5 broken config reload keeps last good settings",
        "puzzle-embed",
        "--lib",
        "settings::tests::invalid_reload_keeps_previous_values",
    );
}

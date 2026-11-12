//! M6.4h acceptance for recorded macOS Option-key input (§6.8).
//!
//! The private recording stays under target/. Its input fields are copied
//! into named tests so routine acceptance never reads a user's shell output.
use super::super::report;

/// Runs exactly one desktop adapter test, failing if its name went stale.
fn exact_test(name: &str) -> Result<(), String> {
    // Cargo returns success for a filter that selects zero tests. Matching
    // the result line makes a renamed regression visible to acceptance.
    let args = ["test", "-p", "puzzle-embed", "--lib", name, "--", "--exact"];
    let output = crate::commands::capture("cargo", &args)?;
    if output.contains(&format!("test {name} ... ok")) {
        Ok(())
    } else {
        Err(format!("{name} did not run"))
    }
}

/// Reports the recorded layout, side policy, dead key and motion contracts.
pub(super) fn report_option_key(passed: &mut bool) {
    // The table carries entry numbers from both physical sides and checks
    // the legacy and kitty flag-one paths independently for each character.
    // This guards layout text against the old Alt suppression that made
    // German Option+7 disappear from the shell.
    report(
        "M6.4h recorded Option text in legacy and kitty flag 1",
        exact_test("keyboard::tests::recorded_option_text_uses_layout_on_both_sides"),
        passed,
    );
    // The base key comes from winit's modifier supplement, not a US layout
    // guess from physical position.
    // Left-only mode must preserve ordinary right-Option typing.
    report(
        "M6.4h left Option-as-Alt uses the recorded base key",
        exact_test("keyboard::tests::option_as_alt_left_routes_base_key"),
        passed,
    );
    report(
        "M6.4h both Option sides can report Alt",
        exact_test("keyboard::tests::option_as_alt_both_routes_base_key"),
        passed,
    );
    // IME Commit can precede the modifier release for a macOS dead key.
    // A modifier-based rejection alone dropped the resulting tilde.
    report(
        "M6.4h Option dead-key commit reaches the PTY once",
        exact_test("keyboard::tests::recorded_option_dead_key_commit_survives_modifier_order"),
        passed,
    );
    // An Option motion has no layout text and must keep Alt for word movement.
    // Treating every Option press as plain text would break this control.
    report(
        "M6.4h Option+Left keeps Alt without text",
        exact_test("keyboard::tests::option_arrow_keeps_alt_without_committed_text"),
        passed,
    );
}

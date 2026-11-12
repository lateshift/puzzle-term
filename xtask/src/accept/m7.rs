//! Ordered M7 acceptance inventory, extended only as each task is implemented.
//!
//! Pure shortcut and adapter tables precede the later browser/bridge work.
//! Named test witnesses must actually execute: Cargo also succeeds with an
//! empty filter, which would conceal an accidentally removed contract test.
use super::{bridge, report};
/// Live-window parity and headless CLI evidence before the desktop review.
mod desktop;
/// Platform input commands must exist before DOM listeners can submit them.
mod dom;
/// Deferred services let the Send core hand work to a single-thread JS host.
mod host;
/// Optimized browser/native budgets and the full shipped engine size.
mod measurements;

/// Executes an exact public behavior witness rather than accepting an empty run.
pub(super) fn test(package: &str, name: &str) -> Result<(), String> {
    let mut args = vec!["test", "-p", package];
    // Desktop and xtask are binaries; neutral domain crates expose library targets.
    if !matches!(package, "puzzle-desktop" | "xtask") {
        args.push("--lib");
    }
    args.extend([name, "--", "--exact"]);
    let output = crate::commands::capture("cargo", &args)?;
    // Exact test names are checked in captured output, not just process status.
    // Cargo treats a filter matching no tests as success, which is not evidence.
    // A renamed test must fail its criterion even if other tests stay green.
    if !output.contains(&format!("test {name} ... ok")) {
        return Err(format!("{package} test {name} did not run"));
    }
    Ok(())
}

/// Each named witness pins a distinct shortcut contract from the work order.
const CORE: &[(&str, &str)] = &[
    (
        // Config errors are isolated to one entry so a typo cannot erase valid shortcuts.
        // Future actions must remain discoverable with their milestone availability.
        "keybinding valid/invalid parsing and per-entry reports",
        "parsing_and_per_entry_reports",
    ),
    (
        // Shift used to produce plus must not require two OS-specific user bindings.
        // Exact physical identities still win over layout-aware logical names.
        "US/German plus and physical/logical precedence",
        "layouts_and_physical_precedence",
    ),
    (
        // Held input remains encoder events, preserving kitty release and repeat modes.
        // Zero timeout means indefinitely pending rather than an immediate flush.
        "leader sequences, ordered flush and finite/infinite timeout",
        "sequences_flush_and_timeout",
    ),
    (
        // Unavailable selection must leave Windows Ctrl+C usable as an interrupt.
        // Unconsumed actions must release every original step in sequence order.
        "performable, unconsumed and all prefixes",
        "prefixes_and_composition",
    ),
    (
        // An IME owns its composing keystrokes even when they resemble app shortcuts.
        // Composition must also leave a pre-existing leader unadvanced.
        "no keybinding fires during IME composition",
        "prefixes_and_composition",
    ),
    (
        // Aliases and reordered modifiers identify the same override across layers.
        // Clear and unbind must remove earlier bindings rather than create actions.
        "canonical layer merge, clear and unbind",
        "canonical_layers_and_clearing",
    ),
    (
        // The alias table preserves portable argument payloads and reports unsupported tabs.
        // Representative configs protect interoperability beyond individual aliases.
        "15+ alias lines and unsupported-action reports",
        "alias_spellings_and_real_world_lines",
    ),
    (
        // Mac natural editing sends literal control bytes independently of kitty mode.
        // Clipboard tables are tested for all operating systems on every host.
        "platform defaults preserve M6 editing and clipboard contract",
        "platform_defaults_preserve_m6_bytes",
    ),
];

/// Shortcut contracts report separately so one failure cannot hide another.
fn core(passed: &mut bool) {
    for (criterion, name) in CORE {
        // Each row pins a distinct §14.9 rule through the same core as live input.
        report(
            criterion,
            test("puzzle-term", &format!("keybind::tests::{name}")),
            passed,
        );
    }
}

/// Session checks cover transport ordering that a standalone matcher cannot see.
fn session(passed: &mut bool) {
    report(
        // A later unbound key cannot overtake an earlier resumed leader in one pump.
        // Policies before and after the built-in must each see an original key once.
        "batched leader flush preserves FIFO and public policy priority",
        test(
            "puzzle-term",
            "session::tests::bindings::shortcut_batch_fifo_and_policy_priority",
        ),
        passed,
    );
    report(
        // A timer must flush a held press before its release under enhanced keyboard mode.
        // Screen/reset actions exercise direct VT state rather than synthesized child output.
        "session timeout and prompt/history/reset actions",
        test(
            "puzzle-term",
            "session::tests::bindings::shortcut_timeout_reload_and_core_actions",
        ),
        passed,
    );
    report(
        // Malformed user configuration must report an error instead of panicking.
        // The property explores Unicode, delimiters and bounded external payloads.
        "arbitrary keybinding text never panics",
        test(
            "puzzle-term",
            "keybind::tests::arbitrary_binding_text_never_panics",
        ),
        passed,
    );
}

/// Direct generic witnesses prevent Session integration from hiding bus defects.
fn policies() -> Result<(), String> {
    for name in [
        "borrowed_priority_rewrite_and_veto",
        "resumed_fifo_suffix_and_unknown_name",
        // Missing trusted targets must warn before their debug assertion.
        "unknown_resume_target_warns_and_asserts_in_debug",
        "hook_resume_fifo_and_later_handler",
        "borrowed_resume_cascade_limit_and_chain",
    ] {
        // These use a fixture domain, so no terminal matcher can compensate
        // for incorrect bus priority, FIFO, continuation or cascade behavior.
        test("puzzle-bus", &format!("bus::tests::borrowed::{name}"))?;
    }
    Ok(())
}

/// Raw adapter witnesses prove both metadata and resulting wire bytes.
/// US/German inputs distinguish produced text from the physical US scalar.
/// The keypad witness retains a comma in normal mode while checking the
/// independent SS3 and kitty identities. These are synthetic layout tables;
/// they require neither OS layout switching nor a user recording.
fn encoder(passed: &mut bool) {
    for (label, name) in [
        (
            "kitty alternates on US/German layouts, flags 4 and 4|8",
            "kitty_alternates_us_and_german",
        ),
        (
            "keypad normal/application/kitty with comma decimal",
            "keypad_normal_application_and_kitty",
        ),
    ] {
        // A synthetic RawKey crosses the actual native adapter before encoding.
        // This catches physical/layout confusion that encoder-only tests miss.
        report(
            label,
            test(
                "puzzle-embed",
                &format!("keyboard::tests::conformance::{name}"),
            ),
            passed,
        );
    }
}

/// Runs the implemented task range; browser criteria arrive after the desktop STOP.
pub(super) fn run() -> bool {
    // Accumulate failures so a parser failure cannot hide a later wire defect.
    // The common gate includes native and wasm compilation plus source rules.
    // Human review follows the mechanical inventory and never changes its exit code.
    let mut passed = true;
    report(
        "native and wasm check",
        crate::commands::check(),
        &mut passed,
    );
    core(&mut passed);
    encoder(&mut passed);
    desktop::run(&mut passed);
    session(&mut passed);
    // Resume is trusted domain metadata, not a keybinding-only special case.
    // Keep direct bus behavior separate from the end-to-end Session witnesses.
    report(
        "borrowed-policy priority, resume FIFO and cascade contracts",
        policies(),
        &mut passed,
    );
    bridge::run(&mut passed);
    dom::core(&mut passed);
    host::run(&mut passed);
    dom::browser(&mut passed);
    measurements::run(&mut passed);
    // Reviewer confirmations are historical facts, not mechanical self-certification (R15).
    // Their full evidence moves to the M7 archive when the milestone closes (§19).
    println!(
        "REVIEW M7.7/M7.8 and review-fix comment quality: confirmed by Claude 2026-10-03 (1b21ab0 and earlier); README.md / README.md"
    );
    // The user confirmed Chrome behavior; synthetic frames do not replace that approval.
    // The user waived Safari approval on 2026-10-03; that is not a Safari confirmation.
    println!(
        "REVIEW M7.8 Chrome: confirmed by user 2026-10-03; Safari approval waived by user 2026-10-03; reproduction steps in README.md"
    );
    // Full security and input-credit reads were independently confirmed by the reviewer.
    println!(
        "REVIEW M7.4 bridge security/comments and input-credit re-check: confirmed by Claude 2026-10-03; README.md / README.md"
    );
    passed
}

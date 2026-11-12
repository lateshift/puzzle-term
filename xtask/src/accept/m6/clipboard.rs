//! Independent M6.4i checks for host clipboard and PTY ordering (§13.1).
//!
//! The criterion lines separate protocol behavior from host integration.
//! A green table of default bindings alone would not prove that a live
//! paste reads system text and reaches the typed Session command. Likewise,
//! a working paste could still leave Cmd+V producing an extra literal `v`
//! or let a Ctrl+C copy shortcut swallow the child's interrupt byte.
//!
//! The Super encoder check covers every ASCII letter as well as an arrow
//! and a Shift chord. These keys take different kitty wire families, so a
//! single Cmd+V test would miss a changed modifier bit on functional keys.
//! The platform-table checks run all three operating-system tables on each
//! developer machine; Windows and Linux behavior cannot wait for M13.
//!
//! Natural editing actions are literal host bindings. Their bytes are
//! checked independently of active kitty flags because sending `\x01` is
//! semantically different from synthesizing a Ctrl+A key report. The
//! standard editing-key table covers the opposite path: keys without host
//! bindings must retain the xterm forms their shells expect.
//!
//! MemoryClipboard proves that headless use stays deterministic while the
//! native provider is installed only in the live window. Mode 2004 is
//! sampled when Paste dispatches, so an application that toggles bracketed
//! paste cannot be given markers from a stale keydown-time snapshot.
//!
//! The real-child writer case enqueues ten megabytes before a key and
//! terminal query replies. The child sleeps before reading, making a
//! synchronous PTY writer block. The acceptance line checks key enqueue
//! latency, ongoing Session frame turns, and the child's complete byte
//! capture after it resumes. It does not claim impossible 50-ms delivery
//! behind ten megabytes of earlier input; the order is intentional.
//!
//! OSC 52 is a separate trust direction from user paste. Its write must
//! cross the policy bus before reaching the host clipboard, while a read
//! query must not call the host provider or answer with private text.
//! Keeping those as a distinct criterion makes a policy regression visible
//! even when Cmd+V itself still works.
use super::super::report;

/// Requires a named test result, because Cargo succeeds with zero matches.
fn exact(package: &str, target: &str, name: &str) -> Result<(), String> {
    let args = ["test", "-p", package, target, name, "--", "--exact"];
    let output = crate::commands::capture("cargo", &args)?;
    if output.contains(&format!("test {name} ... ok")) {
        Ok(())
    } else {
        Err(format!("{name} did not run"))
    }
}

/// Keeps the exact-output guard beside each acceptance label.
fn check(passed: &mut bool, label: &str, package: &str, target: &str, name: &str) {
    report(label, exact(package, target, name), passed);
}

/// Reports each M6.4i contract even when another one fails.
pub(super) fn report_clipboard(passed: &mut bool) {
    check(
        passed,
        "M6.4i legacy Super chords suppressed, kitty modifiers retained",
        "puzzle-vt",
        "--lib",
        "input::tests::super_chords_are_host_only_in_legacy_mode",
    );
    check(
        passed,
        "M6.4i platform paste and performable copy defaults",
        "puzzle-embed",
        "--lib",
        "keyboard::bindings::tests::platform_paste_and_performable_copy_table",
    );
    check(
        passed,
        "M6.4i macOS natural editing bindings",
        "puzzle-embed",
        "--lib",
        "keyboard::bindings::tests::mac_natural_text_editing_table",
    );
    check(
        passed,
        "M6.4i Windows and Linux editing encoder table",
        "puzzle-embed",
        "--lib",
        "keyboard::bindings::tests::windows_linux_standard_editing_keys",
    );
    check(
        passed,
        "M6.4i MemoryClipboard paste obeys mode 2004",
        "puzzle-term",
        "--lib",
        "session::tests::clipboard::memory_clipboard_paste_obeys_bracketed_mode",
    );
    check(
        passed,
        "M6.4i ten-megabyte paste keeps key, replies and frame turns",
        "puzzle-embed",
        "--lib",
        "pty::tests::ten_megabyte_paste_keeps_input_order_and_frame_turns",
    );
    check(
        passed,
        "M6.4i OSC 52 writes, reads denied",
        "puzzle-term",
        "--lib",
        "session::tests::clipboard::osc52_write_reaches_clipboard_and_read_is_denied",
    );
}

//! M5.6 accepts each independently testable input protocol family (§6.8).
//!
//! Keyboard, mouse and paste failures should not obscure each other: a
//! terminal can have correct arrow keys while an unsafe pasted delimiter
//! still escapes. The named unit cases follow the public pure encoders,
//! and `vt_test` verifies they actually executed instead of treating an
//! empty Cargo test filter as success (R14).
use super::{report, vt_test};

/// Reports the M5.6 keyboard, mouse and paste criteria separately.
pub(super) fn report_input(passed: &mut bool) {
    // Both xterm's §6.8 rows and kitty's legacy compatibility forms are
    // needed because full-screen applications negotiate modes at runtime.
    report(
        "legacy xterm key table",
        vt_test("input::tests::legacy_cursor_table")
            .and_then(|()| vt_test("input::tests::legacy_function_table"))
            .and_then(|()| vt_test("input::tests::legacy_control_and_keypad_table"))
            .and_then(|()| vt_test("input::tests::legacy_ctrl_ascii_table")),
        passed,
    );
    // A screen-local stack protects the invoking shell when an editor
    // enters alternate mode; enhanced forms retain published key codes.
    report(
        "kitty flags, stacks and key tables",
        vt_test("input::tests::kitty_flag_stack_is_screen_local_and_bounded")
            .and_then(|()| vt_test("input::tests::kitty_functional_code_table"))
            .and_then(|()| vt_test("input::tests::kitty_enhancement_forms"))
            .and_then(|()| vt_test("input::tests::kitty_all_published_named_pua_rows")),
        passed,
    );
    // Mouse tracking and wire encoding are orthogonal: both must match
    // the xterm/DEC table for a remote application to interpret clicks.
    report(
        "mouse tracking and encoding table",
        vt_test("input::tests::mouse::mouse_encoding_table")
            .and_then(|()| vt_test("input::tests::mouse::mouse_extended_modes_table")),
        passed,
    );
    // Paste security is a §16 boundary even with bracketed mode off.
    report(
        "focus and paste sanitation",
        vt_test("input::tests::mouse::focus_and_paste_sanitise_hostile_input").and_then(|()| {
            vt_test("input::tests::mouse::paste_without_brackets_still_removes_controls")
        }),
        passed,
    );
}

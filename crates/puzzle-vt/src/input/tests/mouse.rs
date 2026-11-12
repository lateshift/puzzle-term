//! Mouse, focus and paste byte tables from §6.8 and xterm ctlseqs.
//!
//! These cases exercise tracking policy and wire syntax independently.
use super::super::*;

/// The §6.8 mouse mode table filters before choosing wire syntax.
#[test]
fn mouse_encoding_table() {
    let mut event = MouseEvent {
        kind: MouseKind::Press,
        button: 0,
        column: 3,
        row: 4,
        pixel_x: 30,
        pixel_y: 40,
        modifiers: Modifiers(0),
        button_held: true,
        click_count: 1,
    };
    assert!(encode_mouse(event, MouseTracking::None, MouseEncoding::Sgr, true).is_empty());
    assert_eq!(
        encode_mouse(event, MouseTracking::Normal, MouseEncoding::Default, true),
        b"\x1b[M $%"
    );
    assert_eq!(
        encode_mouse(event, MouseTracking::Normal, MouseEncoding::Sgr, true),
        b"\x1b[<0;4;5M"
    );
    assert_eq!(
        encode_mouse(event, MouseTracking::Normal, MouseEncoding::SgrPixels, true),
        b"\x1b[<0;31;41M"
    );
    assert_eq!(
        encode_mouse(event, MouseTracking::Normal, MouseEncoding::Urxvt, true),
        b"\x1b[32;4;5M"
    );
    event.kind = MouseKind::Release;
    assert_eq!(
        encode_mouse(event, MouseTracking::Normal, MouseEncoding::Sgr, true),
        b"\x1b[<0;4;5m"
    );
    assert!(encode_mouse(event, MouseTracking::X10, MouseEncoding::Sgr, true).is_empty());
    event.kind = MouseKind::Move;
    assert!(encode_mouse(event, MouseTracking::Normal, MouseEncoding::Sgr, true).is_empty());
    assert_eq!(
        encode_mouse(event, MouseTracking::ButtonMotion, MouseEncoding::Sgr, true),
        b"\x1b[<32;4;5M"
    );
    event.modifiers = Modifiers(Modifiers::SHIFT);
    assert!(encode_mouse(event, MouseTracking::AnyMotion, MouseEncoding::Sgr, true).is_empty());
}

/// UTF-8, wheel, extra button and pixel reports cover the remaining rows.
#[test]
fn mouse_extended_modes_table() {
    let mut event = MouseEvent {
        kind: MouseKind::Wheel,
        button: 64,
        column: 240,
        row: 1,
        pixel_x: 480,
        pixel_y: 2,
        modifiers: Modifiers(0),
        button_held: false,
        click_count: 1,
    };
    assert!(encode_mouse(event, MouseTracking::Normal, MouseEncoding::Default, false).is_empty());
    assert_eq!(
        encode_mouse(event, MouseTracking::Normal, MouseEncoding::Utf8, false),
        b"\x1b[M`\xc4\x91\"".to_vec()
    );
    assert_eq!(
        encode_mouse(event, MouseTracking::Normal, MouseEncoding::Sgr, false),
        b"\x1b[<64;241;2M"
    );
    event.button = 128;
    event.column = 1;
    event.modifiers = Modifiers(Modifiers::CTRL);
    assert_eq!(
        encode_mouse(event, MouseTracking::Normal, MouseEncoding::Sgr, false),
        b"\x1b[<144;2;2M"
    );
    event.kind = MouseKind::Move;
    assert!(
        encode_mouse(
            event,
            MouseTracking::ButtonMotion,
            MouseEncoding::Sgr,
            false
        )
        .is_empty()
    );
    assert!(
        !encode_mouse(
            event,
            MouseTracking::AnyMotion,
            MouseEncoding::SgrPixels,
            false
        )
        .is_empty()
    );
}

/// The §16 paste boundary strips forged close markers and C0 commands.
#[test]
fn focus_and_paste_sanitise_hostile_input() {
    let modes = InputModes {
        focus_reports: true,
        bracketed_paste: true,
        ..InputModes::default()
    };
    assert_eq!(encode_focus(true, modes), b"\x1b[I");
    assert_eq!(encode_focus(false, modes), b"\x1b[O");
    assert_eq!(
        encode_paste("a\r\nb\nc\x1b[201~d\x07", modes),
        b"\x1b[200~a\rb\rcd\x1b[201~"
    );
}

/// Sanitation is independent of bracketing and disabled focus is silent.
#[test]
fn paste_without_brackets_still_removes_controls() {
    let modes = InputModes::default();
    assert!(encode_focus(true, modes).is_empty());
    assert_eq!(encode_paste("a\x1b[201~\nb\x00\t", modes), b"a\rb\t");
}

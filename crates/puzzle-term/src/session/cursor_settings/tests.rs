//! Cursor precedence from §14.2, independent of VT's queryable state.
//!
//! A table covers all seven DECSCUSR codes because treating zero as
//! xterm's blinking block would bypass the configured user preference.
//! The test also toggles mode 12 and explicit blink policy: those are
//! separate sources of a timer, not geometry variants.

use super::{CursorBlink, CursorColor, CursorSettings, CursorStyle};
use puzzle_vt::Terminal;

/// Tests every configured shape against DECSCUSR 0–6 and mode 12.
#[test]
fn precedence_table_covers_shape_blink_and_app_permission() {
    let app_shapes = [
        None,
        Some(CursorStyle::Block),
        Some(CursorStyle::Block),
        Some(CursorStyle::Underline),
        Some(CursorStyle::Underline),
        Some(CursorStyle::Bar),
        Some(CursorStyle::Bar),
    ];
    for configured in [
        CursorStyle::Block,
        CursorStyle::Bar,
        CursorStyle::Underline,
        CursorStyle::BlockHollow,
    ] {
        for allow in [false, true] {
            for (code, app_shape) in app_shapes.into_iter().enumerate() {
                for mode in [false, true] {
                    for policy in [CursorBlink::Auto, CursorBlink::On, CursorBlink::Off] {
                        let settings = CursorSettings {
                            style: configured,
                            blink: policy,
                            allow_app_style: allow,
                            color: CursorColor::Theme,
                        };
                        let actual = settings.presentation(code as u8, mode);
                        let wanted_shape = if allow {
                            app_shape.unwrap_or(configured)
                        } else {
                            configured
                        };
                        let wanted_blink = match policy {
                            CursorBlink::On => true,
                            CursorBlink::Off => false,
                            CursorBlink::Auto => mode && !(allow && code != 0 && code % 2 == 0),
                        };
                        assert_eq!(actual.style, wanted_shape, "code={code}, allow={allow}");
                        assert_eq!(actual.blinks, wanted_blink, "code={code}, mode={mode}");
                    }
                }
            }
        }
    }
    assert!(CursorSettings::default().allow_app_style);
}

/// A drawn override does not falsify the DECRQSS report to the application.
#[test]
fn decrqss_returns_ignored_app_request_and_reset_uses_settings() {
    let settings = CursorSettings {
        style: CursorStyle::Bar,
        blink: CursorBlink::Off,
        allow_app_style: false,
        color: CursorColor::Theme,
    };
    let mut terminal = Terminal::new(8, 1, 0, 0);
    // xterm ctlseqs: CSI Ps SP q selects DECSCUSR; DCS $ q SP q
    // asks for that stored request even if rendering ignores it.
    terminal.advance(b"\x1b[4 q\x1bP$q q\x1b\\");
    assert_eq!(terminal.replies, b"\x1bP1$r4 q\x1b\\");
    assert_eq!(
        settings.presentation(terminal.cursor_style, true).style,
        CursorStyle::Bar
    );
    terminal.advance(b"\x1b[!p");
    assert_eq!(terminal.cursor_style, 0);
    assert_eq!(settings.presentation(0, true).style, CursorStyle::Bar);
    terminal.advance(b"\x1bc");
    assert_eq!(terminal.cursor_style, 0);
    assert_eq!(settings.presentation(0, true).style, CursorStyle::Bar);
}

/// Rejects malformed hexadecimal strings before they reach GPU color data.
#[test]
fn cursor_color_validation_is_exact() {
    assert_eq!(CursorColor::parse("theme"), Ok(CursorColor::Theme));
    assert_eq!(CursorColor::parse("inverse"), Ok(CursorColor::Inverse));
    assert_eq!(
        CursorColor::parse("#1e2F80"),
        Ok(CursorColor::Fixed([30, 47, 128]))
    );
    for invalid in ["#123", "#12345678", "#12ggff", "red"] {
        assert!(CursorColor::parse(invalid).is_err(), "{invalid}");
    }
}

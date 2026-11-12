//! Kitty OSC 21: https://sw.kovidgoyal.net/kitty/color-stack/
use super::*;

/// A set and query in one packet observes current state; unknown keys are safe Base64.
#[test]
fn ordered_multi_key_and_shared_xterm_state() {
    for end in ["\x07", "\x1b\\"] {
        let mut term = Terminal::new(4, 2, 0, 0);
        feed(
            &mut term,
            "21;foreground=Forest Green;foreground=?;17=#123456;17=?;cursor=;cursor=?;nonsense=?;transparent_background_color1=?",
            end,
        );
        assert_eq!(std::mem::take(&mut term.replies), format!("\x1b]21;foreground=rgb:2222/8b8b/2222;17=rgb:1212/3434/5656;cursor=;unknown=bm9uc2Vuc2U;unknown=dHJhbnNwYXJlbnRfYmFja2dyb3VuZF9jb2xvcjE{end}").as_bytes());
        feed(&mut term, "10;?", end);
        assert_eq!(
            std::mem::take(&mut term.replies),
            format!("\x1b]10;rgb:2222/8b8b/2222{end}").as_bytes()
        );
        feed(&mut term, "17;#789abc", end);
        feed(&mut term, "21;selection_background=?", end);
        assert_eq!(
            std::mem::take(&mut term.replies),
            format!("\x1b]21;selection_background=rgb:7878/9a9a/bcbc{end}").as_bytes()
        );
        feed(&mut term, "21;foreground;cursor;selection_background", end);
        assert_eq!(
            term.colors
                .override_value(ColorSlot::Dynamic(DynamicColor::Foreground)),
            None
        );
        assert_eq!(
            term.colors
                .override_value(ColorSlot::Dynamic(DynamicColor::Cursor)),
            None
        );
        feed(&mut term, "21;foreground=#badhex;foreground=?", end);
        assert_eq!(
            std::mem::take(&mut term.replies),
            format!("\x1b]21;foreground=rgb:e5e5/e5e5/e5e5{end}").as_bytes()
        );
    }
}

/// All xterm aliases, dynamic series and kitty-only roles are cleared by RIS.
#[test]
fn aliases_successive_dynamic_and_ris() {
    let mut term = Terminal::new(4, 2, 0, 0);
    feed(&mut term, "4;256;#123456", "\x07");
    feed(&mut term, "5;0;?", "\x07");
    assert_eq!(
        std::mem::take(&mut term.replies),
        b"\x1b]5;0;rgb:1212/3434/5656\x07"
    );
    feed(&mut term, "10;red;blue;green", "\x07");
    assert_eq!(
        term.colors.get(ColorSlot::Dynamic(DynamicColor::Cursor)),
        ColorValue::Rgb([0, 255, 0])
    );
    feed(
        &mut term,
        "21;cursor_text=red;visual_bell=blue;selection_foreground=white;0=red",
        "\x07",
    );
    term.advance(b"\x1bc");
    for n in 10..=19 {
        assert_eq!(
            term.colors.override_value(ColorSlot::Dynamic(
                DynamicColor::from_osc(n).expect("validated colour fixture")
            )),
            None
        );
    }
    for role in [DynamicColor::CursorText, DynamicColor::VisualBell] {
        assert_eq!(term.colors.override_value(ColorSlot::Dynamic(role)), None);
    }
    assert_eq!(term.colors.override_value(ColorSlot::Palette(0)), None);
    assert_eq!(
        term.colors.override_value(ColorSlot::Special(
            puzzle_vt::color_table::SpecialColor::Bold
        )),
        None
    );
}

/// A dynamic palette assignment is forbidden and a malformed field cannot reset it.
#[test]
fn palette_rejects_dynamic_and_unknown_names_cannot_inject() {
    let mut term = Terminal::new(4, 2, 0, 0);
    feed(&mut term, "21;1=red;1=;1=?;foreground=rgbi:NaN/0/0", "\x07");
    assert_eq!(
        std::mem::take(&mut term.replies),
        b"\x1b]21;1=rgb:ffff/0000/0000\x07"
    );
    feed(&mut term, "21;ä=?", "\x07");
    assert_eq!(
        std::mem::take(&mut term.replies),
        b"\x1b]21;unknown=w6Q\x07"
    );
}

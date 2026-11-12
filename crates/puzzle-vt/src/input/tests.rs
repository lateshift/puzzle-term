//! Wire tables protect app-facing keyboard, mouse and paste bytes (§6.8).
//!
//! Xterm ctlseqs names the legacy cursor and function-key families.
//! Kitty's published keyboard protocol supplies the progressive flags,
//! functional-key codes, legacy C0 map and stack controls tested here.
use super::*;
use crate::Terminal;

/// The §6.8 arrow, Home and End rows differ only in final and DECCKM.
#[test]
fn legacy_cursor_table() {
    let entries = [
        (Key::Up, "A"),
        (Key::Down, "B"),
        (Key::Right, "C"),
        (Key::Left, "D"),
        (Key::Home, "H"),
        (Key::End, "F"),
    ];
    for (key, final_byte) in entries {
        let mut modes = InputModes::default();
        let mut event = KeyEvent::press(key);
        assert_eq!(
            encode_key(&event, modes),
            format!("\x1b[{final_byte}").as_bytes()
        );
        modes.application_cursor = true;
        assert_eq!(
            encode_key(&event, modes),
            format!("\x1bO{final_byte}").as_bytes()
        );
        event.modifiers = Modifiers(Modifiers::CTRL);
        assert_eq!(
            encode_key(&event, modes),
            format!("\x1b[1;5{final_byte}").as_bytes()
        );
    }
}

/// Kitty's legacy functional-key table lists each editing and F key.
#[test]
fn legacy_function_table() {
    let entries = [
        (Key::Insert, "\x1b[2~"),
        (Key::Delete, "\x1b[3~"),
        (Key::PageUp, "\x1b[5~"),
        (Key::PageDown, "\x1b[6~"),
        (Key::Function(1), "\x1bOP"),
        (Key::Function(2), "\x1bOQ"),
        (Key::Function(3), "\x1bOR"),
        (Key::Function(4), "\x1bOS"),
        (Key::Function(5), "\x1b[15~"),
        (Key::Function(6), "\x1b[17~"),
        (Key::Function(7), "\x1b[18~"),
        (Key::Function(8), "\x1b[19~"),
        (Key::Function(9), "\x1b[20~"),
        (Key::Function(10), "\x1b[21~"),
        (Key::Function(11), "\x1b[23~"),
        (Key::Function(12), "\x1b[24~"),
    ];
    for (key, bytes) in entries {
        assert_eq!(
            encode_key(&KeyEvent::press(key), InputModes::default()),
            bytes.as_bytes()
        );
    }
}

/// The §6.8 C0, Alt, keypad and LNM rows use separate mode switches.
#[test]
fn legacy_control_and_keypad_table() {
    let mut modes = InputModes::default();
    let entries = [
        (Key::Enter, b"\r".as_slice()),
        (Key::Tab, b"\t"),
        (Key::Backspace, b"\x7f"),
        (Key::Escape, b"\x1b"),
        (Key::Char('@'), b"@"),
    ];
    for (key, bytes) in entries {
        assert_eq!(encode_key(&KeyEvent::press(key), modes), bytes);
    }
    modes.application_keypad = true;
    assert_eq!(
        encode_key(&KeyEvent::press(Key::Keypad(0)), modes),
        b"\x1bOp"
    );
    assert_eq!(
        encode_key(&KeyEvent::press(Key::Keypad(9)), modes),
        b"\x1bOy"
    );
    for (ch, final_byte) in [('+', 'k'), ('-', 'm'), ('*', 'j'), ('/', 'o'), ('.', 'n')] {
        assert_eq!(
            encode_key(&KeyEvent::press(Key::KeypadOp(ch)), modes),
            format!("\x1bO{final_byte}").as_bytes()
        );
    }
    assert_eq!(
        encode_key(&KeyEvent::press(Key::KeypadEnter), modes),
        b"\x1bOM"
    );
    modes.newline = true;
    assert_eq!(encode_key(&KeyEvent::press(Key::Enter), modes), b"\r\n");
    modes.alt_sends_escape = true;
    let mut alt = KeyEvent::press(Key::Char('x'));
    alt.modifiers = Modifiers(Modifiers::ALT);
    assert_eq!(encode_key(&alt, modes), b"\x1bx");
    alt.key = Key::Tab;
    alt.modifiers = Modifiers(Modifiers::SHIFT);
    assert_eq!(encode_key(&alt, modes), b"\x1b[Z");
}

/// Legacy xterm has no Super field, while kitty encodes it as modifier 9.
#[test]
fn super_chords_are_host_only_in_legacy_mode() {
    let legacy = InputModes::default();
    let kitty = InputModes {
        kitty_flags: 1,
        ..legacy
    };
    for ch in 'a'..='z' {
        let mut event = KeyEvent::press(Key::Char(ch));
        event.modifiers = Modifiers(Modifiers::SUPER);
        assert!(encode_key(&event, legacy).is_empty(), "{ch}");
        assert_eq!(
            encode_key(&event, kitty),
            format!("\x1b[{};9u", u32::from(ch)).as_bytes(),
            "{ch}"
        );
    }
    for (key, mods, expected) in [
        (
            Key::Char('v'),
            Modifiers::SUPER | Modifiers::SHIFT,
            "\x1b[118;10u",
        ),
        (Key::Left, Modifiers::SUPER, "\x1b[1;9D"),
    ] {
        let mut event = KeyEvent::press(key);
        event.modifiers = Modifiers(mods);
        assert!(encode_key(&event, legacy).is_empty());
        assert_eq!(encode_key(&event, kitty), expected.as_bytes());
    }
}

/// The kitty table makes all five enhancement bits queryable per screen.
#[test]
fn kitty_flag_stack_is_screen_local_and_bounded() {
    let mut term = Terminal::new(4, 2, 0, 0);
    term.advance(b"\x1b[>1u\x1b[>2u\x1b[?u");
    assert_eq!(term.replies, b"\x1b[?2u");
    term.advance(b"\x1b[?1049h\x1b[>31u\x1b[?u");
    assert_eq!(term.replies.last(), Some(&b'u'));
    assert_eq!(term.kitty_keys.flags(term.active), 31);
    term.advance(b"\x1b[?1049l\x1b[<u\x1b[?u");
    assert_eq!(term.kitty_keys.flags(term.active), 1);
    term.advance(b"\x1b[=4;2u");
    assert_eq!(term.kitty_keys.flags(term.active), 5);
    term.advance(b"\x1b[=1;3u");
    assert_eq!(term.kitty_keys.flags(term.active), 4);
    for _ in 0..32 {
        term.advance(b"\x1b[>31u");
    }
    for _ in 0..16 {
        term.advance(b"\x1b[<u");
    }
    assert_eq!(term.kitty_keys.flags(term.active), 0);
    term.advance(b"\x1bc");
    assert_eq!(term.kitty_keys.flags(term.active), 0);
}

/// Nvim's alternate-screen negotiation must not overwrite primary flags.
#[test]
fn kitty_ctrl_d_survives_alternate_screen_switch() {
    let mut term = Terminal::new(80, 24, 0, 0);
    let mut event = KeyEvent::press(Key::Char('d'));
    event.modifiers = Modifiers(Modifiers::CTRL);
    term.advance(b"\x1b[>1u\x1b[?1049h\x1b[>3u");
    assert_eq!(
        encode_key(&event, InputModes::from_terminal(&term)),
        b"\x1b[100;5u"
    );
    term.advance(b"\x1b[?1049l");
    assert_eq!(term.kitty_keys.flags(term.active), 1);
    assert_eq!(
        encode_key(&event, InputModes::from_terminal(&term)),
        b"\x1b[100;5u"
    );
    term.advance(b"\x1b[?1049h\x1b[<u");
    assert_eq!(term.kitty_keys.flags(term.active), 0);
    assert_eq!(
        encode_key(&event, InputModes::from_terminal(&term)),
        b"\x04"
    );
}

/// The kitty PUA table is continuous for F13–35 and keypad/media keys.
#[test]
fn kitty_functional_code_table() {
    let modes = InputModes {
        kitty_flags: 8,
        ..InputModes::default()
    };
    for (key, code) in [
        (Key::Function(13), 57376),
        (Key::Function(35), 57398),
        (Key::Keypad(0), 57399),
        (Key::Keypad(9), 57408),
        (Key::KeypadEnter, 57414),
        (Key::Named(NamedKey::MediaPlay), 57428),
        (Key::Named(NamedKey::IsoLevel5Shift), 57454),
    ] {
        assert_eq!(
            encode_key(&KeyEvent::press(key), modes),
            format!("\x1b[{code}u").as_bytes()
        );
    }
}

/// Each named key must map to its independently published kitty code.
///
/// This table transcribes the functional-key definitions at
/// https://sw.kovidgoyal.net/kitty/keyboard-protocol/#functional-key-definitions.
/// Comparing named variants with literal numbers catches an adapter or
/// encoder mapping error that a raw numeric round trip cannot detect.
const KITTY_NAMED_CASES: &[(NamedKey, u32)] = &[
    (NamedKey::CapsLock, 57358),
    (NamedKey::ScrollLock, 57359),
    (NamedKey::NumLock, 57360),
    (NamedKey::PrintScreen, 57361),
    (NamedKey::Pause, 57362),
    (NamedKey::Menu, 57363),
    (NamedKey::KeypadDecimal, 57409),
    (NamedKey::KeypadDivide, 57410),
    (NamedKey::KeypadMultiply, 57411),
    (NamedKey::KeypadSubtract, 57412),
    (NamedKey::KeypadAdd, 57413),
    (NamedKey::KeypadEqual, 57415),
    (NamedKey::KeypadSeparator, 57416),
    (NamedKey::KeypadLeft, 57417),
    (NamedKey::KeypadRight, 57418),
    (NamedKey::KeypadUp, 57419),
    (NamedKey::KeypadDown, 57420),
    (NamedKey::KeypadPageUp, 57421),
    (NamedKey::KeypadPageDown, 57422),
    (NamedKey::KeypadHome, 57423),
    (NamedKey::KeypadEnd, 57424),
    (NamedKey::KeypadInsert, 57425),
    (NamedKey::KeypadDelete, 57426),
    (NamedKey::KeypadBegin, 57427),
    (NamedKey::MediaPlay, 57428),
    (NamedKey::MediaPause, 57429),
    (NamedKey::MediaPlayPause, 57430),
    (NamedKey::MediaReverse, 57431),
    (NamedKey::MediaStop, 57432),
    (NamedKey::MediaFastForward, 57433),
    (NamedKey::MediaRewind, 57434),
    (NamedKey::MediaNext, 57435),
    (NamedKey::MediaPrevious, 57436),
    (NamedKey::MediaRecord, 57437),
    (NamedKey::LowerVolume, 57438),
    (NamedKey::RaiseVolume, 57439),
    (NamedKey::MuteVolume, 57440),
    (NamedKey::LeftShift, 57441),
    (NamedKey::LeftControl, 57442),
    (NamedKey::LeftAlt, 57443),
    (NamedKey::LeftSuper, 57444),
    (NamedKey::LeftHyper, 57445),
    (NamedKey::LeftMeta, 57446),
    (NamedKey::RightShift, 57447),
    (NamedKey::RightControl, 57448),
    (NamedKey::RightAlt, 57449),
    (NamedKey::RightSuper, 57450),
    (NamedKey::RightHyper, 57451),
    (NamedKey::RightMeta, 57452),
    (NamedKey::IsoLevel3Shift, 57453),
    (NamedKey::IsoLevel5Shift, 57454),
];

/// Exercises every named identity through the complete encoding path.
#[test]
fn kitty_all_published_named_pua_rows() {
    let modes = InputModes {
        kitty_flags: 8,
        ..InputModes::default()
    };
    for &(name, code) in KITTY_NAMED_CASES {
        assert_eq!(name.code(), code, "kitty key {name:?}");
        assert_eq!(
            encode_key(&KeyEvent::press(Key::Named(name)), modes),
            format!("\x1b[{code}u").as_bytes(),
            "kitty key {name:?}"
        );
    }
}

/// The legacy Ctrl row is a table, not a blanket Unicode bit mask.
#[test]
fn legacy_ctrl_ascii_table() {
    // Xterm ctlseqs and kitty's legacy Ctrl table agree on these C0 bytes.
    for (input, code) in [
        (' ', 0),
        ('@', 0),
        ('[', 27),
        ('\\', 28),
        (']', 29),
        ('^', 30),
        ('_', 31),
    ] {
        let mut event = KeyEvent::press(Key::Char(input));
        event.modifiers = Modifiers(Modifiers::CTRL);
        assert_eq!(encode_key(&event, InputModes::default()), [code]);
    }
    for input in b'a'..=b'z' {
        let mut event = KeyEvent::press(Key::Char(char::from(input)));
        event.modifiers = Modifiers(Modifiers::CTRL);
        assert_eq!(
            encode_key(&event, InputModes::default()),
            [input - b'a' + 1]
        );
    }
    let mut shifted = KeyEvent::press(Key::Char('c'));
    shifted.modifiers = Modifiers(Modifiers::CTRL | Modifiers::SHIFT);
    assert_eq!(encode_key(&shifted, InputModes::default()), b"\x03");
}

/// Ctrl+letters use C0 in legacy mode and CSI u under kitty flags 1/31.
#[test]
fn ctrl_letter_encoding_by_kitty_flags() {
    // §6.8's xterm table cannot distinguish Ctrl+Shift without a negotiated
    // extension; kitty's disambiguation flag restores that distinction.
    for ch in 'a'..='z' {
        let mut event = KeyEvent::press(Key::Char(ch));
        event.modifiers = Modifiers(Modifiers::CTRL);
        let code = ch as u32;
        let legacy = [ch as u8 - b'a' + 1];
        assert_eq!(encode_key(&event, InputModes::default()), legacy);
        for flags in [1, 31] {
            let modes = InputModes {
                kitty_flags: flags,
                ..InputModes::default()
            };
            assert_eq!(
                encode_key(&event, modes),
                format!("\x1b[{code};5u").as_bytes()
            );
        }
        event.modifiers = Modifiers(Modifiers::CTRL | Modifiers::SHIFT);
        assert_eq!(encode_key(&event, InputModes::default()), legacy);
        for flags in [1, 31] {
            let modes = InputModes {
                kitty_flags: flags,
                ..InputModes::default()
            };
            assert_eq!(
                encode_key(&event, modes),
                format!("\x1b[{code};6u").as_bytes()
            );
        }
    }
}

/// Kitty event, alternate-key and associated-text flags change subfields.
#[test]
fn kitty_enhancement_forms() {
    let mut event = KeyEvent::press(Key::Char('a'));
    event.modifiers = Modifiers(Modifiers::SHIFT | Modifiers::CTRL);
    event.shifted = Some('A');
    event.base = Some('a');
    event.text = Some("A".to_string());
    let modes = InputModes {
        kitty_flags: 31,
        ..InputModes::default()
    };
    assert_eq!(encode_key(&event, modes), b"\x1b[97:65:97;6;65u");
    event.phase = KeyPhase::Repeat;
    assert_eq!(encode_key(&event, modes), b"\x1b[97:65:97;6:2;65u");
    event.phase = KeyPhase::Release;
    assert_eq!(encode_key(&event, modes), b"\x1b[97:65:97;6:3;65u");
}

/// Mouse and paste wire tables are kept separate from keyboard cases.
#[path = "tests/mouse.rs"]
mod mouse;

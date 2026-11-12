//! RawInput adapter cases without constructing private winit event fields.
//!
//! The window copies winit callbacks into these owned values before any
//! mapping. Exercising this seam gives headless scenarios and later replay
//! the same mode-sensitive command decisions as a native key press.

use super::*;
use puzzle_term::session::OptionAsAlt;
use puzzle_term::vt::input::{InputModes, encode_key};

/// Builds a named raw press with all fields a recording preserves.
fn named(name: &str) -> RawInput {
    RawInput::Key(RawKey {
        logical: RawLogical::Named(name.into()),
        base: RawLogical::Named(name.into()),
        physical: format!("Code{name}"),
        text: None,
        location: "Standard".into(),
        pressed: true,
        repeat: false,
    })
}

/// Every §6.8 named control reaches its platform-neutral VT identity.
#[test]
fn named_keys_reach_the_vt_encoder() {
    for (name, expected) in [
        ("Enter", VtKey::Enter),
        ("Backspace", VtKey::Backspace),
        ("Tab", VtKey::Tab),
        ("Escape", VtKey::Escape),
        ("ArrowUp", VtKey::Up),
        ("ArrowDown", VtKey::Down),
        ("ArrowRight", VtKey::Right),
        ("ArrowLeft", VtKey::Left),
        ("Home", VtKey::Home),
        ("End", VtKey::End),
        ("PageUp", VtKey::PageUp),
        ("PageDown", VtKey::PageDown),
        ("F1", VtKey::Function(1)),
        ("F12", VtKey::Function(12)),
    ] {
        assert_eq!(map_named(name), Some(expected), "{name}");
    }
}

/// One raw identity still follows DECCKM, Ctrl and Alt encoder modes.
#[test]
fn winit_keys_encode_with_current_terminal_modes() {
    for (name, bytes) in [
        ("Enter", b"\r".as_slice()),
        ("Backspace", b"\x7f"),
        ("ArrowUp", b"\x1b[A"),
        ("F1", b"\x1bOP"),
        ("PageDown", b"\x1b[6~"),
    ] {
        assert_encoded(named(name), 0, false, bytes);
    }
    assert_encoded(named("ArrowUp"), 0, true, b"\x1bOA");
    assert_encoded(
        RawInput::Key(RawKey::character("c", None)),
        Modifiers::CTRL,
        false,
        b"\x03",
    );
    assert_encoded(
        RawInput::Key(RawKey::character("x", None)),
        Modifiers::ALT,
        false,
        b"\x1bx",
    );
}

/// Committed layout text wins over the uncomposed logical key.
#[test]
fn committed_layout_text_wins() {
    let mut adapter = Adapter::default();
    let command = adapter.apply(RawInput::Key(RawKey::character("e", Some("é"))));
    let Some(Command::Key(event)) = command else {
        panic!("missing composed key");
    };
    assert_eq!(encode_key(&event, InputModes::default()), "é".as_bytes());
    let multi = adapter.apply(RawInput::Key(RawKey::character("e", Some("e\u{301}"))));
    assert!(matches!(multi, Some(Command::Text(text)) if text == "e\u{301}"));
}

/// Modifier callbacks and IME commits cannot double-send Ctrl+D.
#[test]
fn control_modifier_prevents_duplicate_ime_send() {
    let mut adapter = Adapter::default();
    let _ = adapter.apply(RawInput::Modifiers(Modifiers::CTRL));
    let command = adapter.apply(RawInput::Key(RawKey::character("d", Some("\u{4}"))));
    let Some(Command::Key(event)) = command else {
        panic!("missing Ctrl+D key");
    };
    assert_eq!(encode_key(&event, InputModes::default()), b"\x04");
    assert_eq!(
        encode_key(
            &event,
            InputModes {
                kitty_flags: 3,
                ..InputModes::default()
            }
        ),
        b"\x1b[100;5u"
    );
    assert!(
        adapter
            .apply(RawInput::Ime(RawIme::Commit("\u{4}".into())))
            .is_none()
    );
    let _ = adapter.apply(RawInput::Modifiers(0));
    assert!(
        matches!(adapter.apply(RawInput::Ime(RawIme::Commit("d".into()))), Some(Command::Text(text)) if text == "d")
    );
}

/// C0 and DEL remain key-path-only even after modifier release.
#[test]
fn control_commit_is_rejected_without_modifier_state() {
    let mut adapter = Adapter::default();
    for code in 0..=31_u32 {
        let text = char::from_u32(code).unwrap_or_default().to_string();
        assert!(adapter.apply(RawInput::Ime(RawIme::Commit(text))).is_none());
    }
    assert!(
        adapter
            .apply(RawInput::Ime(RawIme::Commit("\u{7f}".into())))
            .is_none()
    );
    assert!(
        matches!(adapter.apply(RawInput::Ime(RawIme::Commit("é".into()))), Some(Command::Text(text)) if text == "é")
    );
}

/// Sends one raw key through adapter and mode-dependent byte encoder.
fn assert_encoded(input: RawInput, modifiers: u8, application_cursor: bool, expected: &[u8]) {
    let mut adapter = Adapter::default();
    let _ = adapter.apply(RawInput::Modifiers(modifiers));
    let Some(Command::Key(event)) = adapter.apply(input) else {
        panic!("key became text");
    };
    let modes = InputModes {
        application_cursor,
        alt_sends_escape: true,
        ..InputModes::default()
    };
    assert_eq!(encode_key(&event, modes), expected);
}

/// Reconstructs only input fields captured in the user's private recording.
fn option_bytes(ch: &str, base: &str, bits: u8, choice: OptionAsAlt, kitty: u8) -> Vec<u8> {
    let mut adapter = Adapter::new(choice);
    let _ = adapter.apply(RawInput::Modifiers(bits));
    let command = adapter.apply(RawInput::Key(RawKey {
        logical: RawLogical::Character(ch.into()),
        base: RawLogical::Character(base.into()),
        physical: String::new(),
        text: Some(ch.into()),
        location: "Standard".into(),
        pressed: true,
        repeat: false,
    }));
    match command {
        Some(Command::Key(event)) => encode_key(
            &event,
            InputModes {
                kitty_flags: kitty,
                alt_sends_escape: true,
                ..InputModes::default()
            },
        ),
        Some(Command::Text(text)) => text.into_bytes(),
        other => panic!("unexpected Option command: {other:?}"),
    }
}

/// Recorded German-layout input keeps Option-produced text under both modes.
#[test]
fn recorded_option_text_uses_layout_on_both_sides() {
    // Entry numbers identify input rows in private de-layout-after.pzrec;
    // only these neutral fields are copied here, never shell output.
    for (entry, ch, base, bits) in [
        (47, "@", "l", 18),
        (64, "[", "5", 18),
        (73, "]", "6", 18),
        (76, "|", "7", 18),
        (110, "\\", "7", 19),
        (221, "|", "7", 34),
        (248, "{", "8", 34),
        (257, "}", "9", 34),
        (266, "[", "5", 34),
        (275, "]", "6", 34),
        (288, "\\", "7", 35),
        (301, "@", "l", 34),
    ] {
        for kitty in [0, 1] {
            assert_eq!(
                option_bytes(ch, base, bits, OptionAsAlt::None, kitty),
                ch.as_bytes(),
                "recording input entry {entry}, kitty flags {kitty}"
            );
        }
    }
}

/// Left-only Alt leaves the right Option key available for layout text.
#[test]
fn option_as_alt_left_routes_base_key() {
    // Entries 76 and 221 are left/right Option+7 with base key `7`.
    assert_eq!(option_bytes("|", "7", 18, OptionAsAlt::Left, 0), b"\x1b7");
    assert_eq!(option_bytes("|", "7", 34, OptionAsAlt::Left, 0), b"|");
    assert_eq!(
        option_bytes("|", "7", 18, OptionAsAlt::Left, 1),
        b"\x1b[55;3u"
    );
}

/// Both-side Alt uses the unmodified layout key on either physical side.
#[test]
fn option_as_alt_both_routes_base_key() {
    // Entries 76 and 221 share logical `|` but have distinct side bits.
    assert_eq!(option_bytes("|", "7", 18, OptionAsAlt::Both, 0), b"\x1b7");
    assert_eq!(option_bytes("|", "7", 34, OptionAsAlt::Both, 0), b"\x1b7");
    assert_eq!(option_bytes("|", "7", 34, OptionAsAlt::Right, 0), b"\x1b7");
}

/// A dead-key commit is text even if Option has not yet been released.
#[test]
fn recorded_option_dead_key_commit_survives_modifier_order() {
    // Old recording entries 255 and 261 contain left Option then `~` Commit.
    let mut adapter = Adapter::new(OptionAsAlt::None);
    let _ = adapter.apply(RawInput::Modifiers(Modifiers::ALT | LEFT_OPTION));
    assert!(
        matches!(adapter.apply(RawInput::Ime(RawIme::Commit("~".into()))),
        Some(Command::Text(text)) if text == "~")
    );
}

/// A non-text Option chord keeps its Alt modifier for word motion.
#[test]
fn option_arrow_keeps_alt_without_committed_text() {
    // de-option-left.pzrec entries 113/115 carry left Option and ArrowLeft;
    // the arrow has no text even though the preceding modifier bit is set.
    let mut adapter = Adapter::new(OptionAsAlt::None);
    let _ = adapter.apply(RawInput::Modifiers(Modifiers::ALT | LEFT_OPTION));
    let input = RawInput::Key(RawKey {
        logical: RawLogical::Named("ArrowLeft".into()),
        base: RawLogical::Named("ArrowLeft".into()),
        physical: "Code(ArrowLeft)".into(),
        text: None,
        location: "Standard".into(),
        pressed: true,
        repeat: false,
    });
    let Some(Command::Key(event)) = adapter.apply(input) else {
        panic!("Option+Left did not map to a typed key");
    };
    assert_eq!(encode_key(&event, InputModes::default()), b"\x1b[1;3D");
}

/// Layout and keypad protocol tables use the same adapter as the live window.
mod conformance;

/// Release instrumentation crosses the actual queued PTY transport boundary.
mod latency;

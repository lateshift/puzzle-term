//! Platform-default triggers are independent of the build host (§14.9).
use super::*;
use crate::keyboard::raw::{RawKey, RawLogical};
use puzzle_term::keybind::{Action, Keybindings, Source, defaults};
use puzzle_term::vt::input::{InputModes, Key, KeyEvent, Modifiers, encode_key};
/// Command uses the standard protocol Super bit.
const SUPER: u8 = Modifiers::SUPER;
/// Control is primary on Windows and Linux.
const CTRL: u8 = Modifiers::CTRL;
/// Shift distinguishes alternate clipboard gestures.
const SHIFT: u8 = Modifiers::SHIFT;
/// Mac Option arrows use Alt for word movement.
const ALT: u8 = Modifiers::ALT;

/// Pins the original platform table through the shared dry-run surface.
fn resolve(
    platform: Platform,
    raw: &RawKey,
    mods: u8,
    composing: bool,
    selected: bool,
) -> Option<Action> {
    let mut bindings = Keybindings::default();
    bindings.apply(
        &defaults::lines(platform),
        Source::Defaults,
        platform,
        false,
    );
    let mut event = KeyEvent::press(Key::Char('c'));
    event.code = raw
        .physical
        .strip_prefix("Code(")
        .and_then(|s| s.strip_suffix(')'))
        .map(str::to_owned);
    event.modifiers = Modifiers(mods);
    event.composing = composing;
    let action = bindings.test(&event)?.action.clone();
    if matches!(action, Action::Copy(_)) && !selected {
        None
    } else {
        Some(action)
    }
}

/// Makes a physical key without requiring a private winit event object.
fn key(code: &str) -> RawKey {
    RawKey {
        logical: RawLogical::Named(code.to_owned()),
        base: RawLogical::Named(code.to_owned()),
        physical: format!("Code({code})"),
        text: None,
        location: "Standard".into(),
        pressed: true,
        repeat: false,
    }
}

/// Every host table is checked on macOS, Windows and Linux build machines.
#[test]
fn platform_paste_and_performable_copy_table() {
    for (platform, code, mods) in [
        (Platform::Mac, "KeyV", SUPER),
        (Platform::Windows, "KeyV", CTRL),
        (Platform::Windows, "KeyV", CTRL | SHIFT),
        (Platform::Windows, "Insert", SHIFT),
        (Platform::Linux, "KeyV", CTRL | SHIFT),
        (Platform::Linux, "Insert", SHIFT),
    ] {
        assert_eq!(
            resolve(platform, &key(code), mods, false, false),
            Some(Action::Paste)
        );
    }
    // Windows Ctrl+C without selection must retain the interrupt byte.
    let copy = key("KeyC");
    assert_eq!(resolve(Platform::Windows, &copy, CTRL, false, false), None);
    assert_eq!(
        resolve(Platform::Windows, &copy, CTRL, false, true),
        Some(Action::Copy("text".into()))
    );
    let mut ctrl_c = KeyEvent::press(Key::Char('c'));
    ctrl_c.modifiers = Modifiers(CTRL);
    assert_eq!(encode_key(&ctrl_c, InputModes::default()), b"\x03");
    assert_eq!(
        resolve(Platform::Mac, &key("KeyV"), SUPER, true, false),
        None
    );
}

/// Mac line and word motions send exact shell bytes before kitty encoding.
#[test]
fn mac_natural_text_editing_table() {
    for (code, mods, action) in [
        ("ArrowLeft", SUPER, Action::SendText("\x01".into())),
        ("ArrowRight", SUPER, Action::SendText("\x05".into())),
        ("Backspace", SUPER, Action::SendText("\x15".into())),
        ("ArrowLeft", ALT, Action::Esc("b".into())),
        ("ArrowRight", ALT, Action::Esc("f".into())),
    ] {
        assert_eq!(
            resolve(Platform::Mac, &key(code), mods, false, false),
            Some(action)
        );
    }
    // The binding layer acts before the encoder; kitty flags cannot alter
    // these literal readline-compatible bytes.
    assert_eq!(
        Some(Action::SendText("\x01".into())),
        resolve(Platform::Mac, &key("ArrowLeft"), SUPER, false, false)
    );
}

/// The unbound standard editing keys retain xterm's protocol forms.
#[test]
fn windows_linux_standard_editing_keys() {
    for (key, mods, normal, application) in [
        (Key::Home, 0, "\x1b[H", "\x1bOH"),
        (Key::End, 0, "\x1b[F", "\x1bOF"),
        (Key::Left, CTRL, "\x1b[1;5D", "\x1b[1;5D"),
        (Key::Right, CTRL, "\x1b[1;5C", "\x1b[1;5C"),
        (Key::Backspace, CTRL, "\x08", "\x08"),
        (Key::Delete, CTRL, "\x1b[3;5~", "\x1b[3;5~"),
    ] {
        let mut event = KeyEvent::press(key);
        event.modifiers = Modifiers(mods);
        assert_eq!(encode_key(&event, InputModes::default()), normal.as_bytes());
        let app = InputModes {
            application_cursor: true,
            ..InputModes::default()
        };
        assert_eq!(encode_key(&event, app), application.as_bytes());
    }
}

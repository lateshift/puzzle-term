//! Synthetic DOM key contracts checked by the browser's actual event objects.
#![allow(
    clippy::expect_used,
    reason = "browser witnesses fail at their exact setup assertion"
)]
use super::*;
use puzzle_term::vt::input::{InputModes, encode_key};
use wasm_bindgen_test::wasm_bindgen_test;
use web_sys::KeyboardEventInit;

/// Constructs browser-shaped data without depending on the host's layout.
fn event(kind: &str, key: &str, code: &str, modifiers: u8) -> KeyboardEvent {
    let init = KeyboardEventInit::new();
    init.set_key(key);
    init.set_code(code);
    init.set_ctrl_key(modifiers & 4 != 0);
    init.set_alt_key(modifiers & 2 != 0);
    init.set_shift_key(modifiers & 1 != 0);
    init.set_meta_key(modifiers & 8 != 0);
    KeyboardEvent::new_with_keyboard_event_init_dict(kind, &init).expect("DOM key")
}

/// Legacy and enhanced modes use the same identity, not browser keyCode guesses.
#[wasm_bindgen_test]
fn key_table_and_kitty_alternates() {
    let policy = KeyPolicy {
        platform: Platform::Mac,
        option_as_alt: OptionAsAlt::None,
        kitty_flags: 12,
        locked: false,
    };
    let mut keyboard = Keyboard::default();
    // German Z is physically US Y; both alternatives must survive mapping.
    keyboard.set_layout_key("KeyY", "z");
    let mapped = keyboard
        .map(&event("keydown", "Z", "KeyY", 1), policy)
        .expect("mapped");
    assert_eq!(mapped.event.key, Key::Char('z'));
    assert_eq!(mapped.event.shifted, Some('Z'));
    assert_eq!(mapped.event.base, Some('y'));
    assert_eq!(
        encode_key(
            &mapped.event,
            InputModes {
                kitty_flags: 12,
                ..InputModes::default()
            }
        ),
        b"\x1b[122:90:121;2u"
    );
    functional_keys_and_releases(&mut keyboard, policy);
}

/// Keypad text and unrequested releases must retain the legacy contract.
fn functional_keys_and_releases(keyboard: &mut Keyboard, policy: KeyPolicy) {
    for (key, code, bytes) in [
        ("ArrowLeft", "ArrowLeft", b"\x1b[D".as_slice()),
        ("F5", "F5", b"\x1b[15~"),
        ("Enter", "Enter", b"\r"),
        (",", "NumpadDecimal", b","),
        ("Tab", "Tab", b"\t"),
    ] {
        let mapped = keyboard
            .map(
                &event("keydown", key, code, 0),
                KeyPolicy {
                    kitty_flags: 0,
                    ..policy
                },
            )
            .expect("key");
        assert!(!mapped.native_text);
        assert_eq!(encode_key(&mapped.event, InputModes::default()), bytes);
    }
    assert!(
        keyboard
            .map(&event("keyup", "a", "KeyA", 0), policy)
            .is_none()
    );
    assert_eq!(
        keyboard
            .map(
                &event("keyup", "a", "KeyA", 0),
                KeyPolicy {
                    kitty_flags: 2,
                    ..policy
                }
            )
            .expect("release")
            .event
            .phase,
        KeyPhase::Release
    );
}

/// Text-producing Option uses input; configured Alt always encodes the base.
#[wasm_bindgen_test]
fn option_sides_with_and_without_layout_map() {
    for layout_map in [false, true] {
        let mut keyboard = Keyboard::default();
        if layout_map {
            keyboard.set_layout_key("KeyY", "z");
        }
        let policy = KeyPolicy {
            platform: Platform::Mac,
            option_as_alt: OptionAsAlt::Left,
            kitty_flags: 8,
            locked: false,
        };
        keyboard.map(&event("keydown", "Alt", "AltLeft", 2), policy);
        let mapped = keyboard
            .map(&event("keydown", "¥", "KeyY", 2), policy)
            .expect("Alt chord never dropped");
        let expected = if layout_map { b"\x1bz" } else { b"\x1by" };
        assert_eq!(
            encode_key(
                &mapped.event,
                InputModes {
                    alt_sends_escape: true,
                    ..InputModes::default()
                }
            ),
            expected
        );
        assert!(!mapped.native_text);
        keyboard.map(&event("keyup", "Alt", "AltLeft", 0), policy);
        keyboard.map(&event("keydown", "Alt", "AltRight", 2), policy);
        let mapped = keyboard
            .map(&event("keydown", "|", "Digit7", 2), policy)
            .expect("Option text");
        assert!(mapped.native_text);
        assert_eq!(mapped.event.modifiers.0, 0);
        keyboard.reset();
    }
}

/// Browser tab/window shortcuts are native; ordinary shell chords are not.
#[wasm_bindgen_test]
fn browser_reserved_shortcuts() {
    for platform in [Platform::Mac, Platform::Windows, Platform::Linux] {
        for ch in ['w', 't', 'n', 'q'] {
            assert!(reserved(
                Key::Char(ch),
                Modifiers(platform.primary()),
                platform
            ));
        }
        assert!(!reserved(Key::Char('d'), Modifiers(4), platform));
    }
    // The shared catalogue reports unavailable bindings rather than silently
    // treating a browser chrome shortcut as a usable application binding.
    let mut bindings = puzzle_term::keybind::Keybindings::default();
    let reports = bindings.apply(
        &["super+w=ignore".into()],
        puzzle_term::keybind::Source::User,
        Platform::Mac,
        true,
    );
    assert!(
        reports
            .iter()
            .any(|report| report.contains("browser-reserved shortcut"))
    );
}

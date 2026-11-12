//! Synthetic US/German winit-shaped events pin §6.8's kitty and keypad contracts.
//! Expectations follow https://sw.kovidgoyal.net/kitty/keyboard-protocol/:
//! alternate codes are layout Shift and physical US, not two copies of text.
use super::*;

/// Supplies all three identities independently, as winit's supplements do.
fn event(code: &str, base: &str, produced: &str, shift: bool) -> VtKeyEvent {
    let mut adapter = Adapter::default();
    adapter.apply(RawInput::Modifiers(u8::from(shift)));
    let mut raw = RawKey::character(produced, Some(produced));
    raw.base = RawLogical::Character(base.into());
    raw.physical = format!("Code({code})");
    let Some(Command::Key(event)) = adapter.apply(RawInput::Key(raw)) else {
        panic!("missing key {code}");
    };
    event
}

/// Flag 4 enhances only escape reports; flag 8 requests even plain keys.
#[test]
fn kitty_alternates_us_and_german() {
    // German z/y prove that base is physical US, not the layout's unshifted key.
    // Plus on US needs Shift; German plus lives on the BracketRight position.
    for (code, base, output, shift, enhanced) in [
        ("KeyY", "y", "y", false, "121::121u"),
        ("KeyY", "y", "Y", true, "121:89:121;2u"),
        ("KeyY", "z", "z", false, "122::121u"),
        ("KeyY", "z", "Z", true, "122:90:121;2u"),
        ("KeyZ", "y", "Y", true, "121:89:122;2u"),
        ("Equal", "=", "+", true, "61:43:61;2u"),
        ("BracketRight", "+", "+", false, "43::93u"),
        ("Semicolon", "ö", "Ö", true, "246:214:59;2u"),
    ] {
        let key = event(code, base, output, shift);
        assert_eq!(key.code.as_deref(), Some(code));
        for flags in [0, 4] {
            assert_eq!(
                encode_key(
                    &key,
                    InputModes {
                        kitty_flags: flags,
                        ..InputModes::default()
                    }
                ),
                output.as_bytes()
            );
        }
        assert_eq!(
            encode_key(
                &key,
                InputModes {
                    kitty_flags: 12,
                    ..InputModes::default()
                }
            ),
            format!("\x1b[{enhanced}").as_bytes()
        );
    }
    control_alternates();
}

/// Alternate reporting alone must preserve legacy Ctrl bytes.
fn control_alternates() {
    let mut ctrl = event("KeyY", "z", "z", false);
    ctrl.modifiers = Modifiers(4);
    ctrl.text = None;
    assert_eq!(
        encode_key(
            &ctrl,
            InputModes {
                kitty_flags: 4,
                ..InputModes::default()
            }
        ),
        b"\x1a"
    );
    assert_eq!(
        encode_key(
            &ctrl,
            InputModes {
                kitty_flags: 5,
                ..InputModes::default()
            }
        ),
        b"\x1b[122::121;5u"
    );
}

/// Every keypad row keeps its KP code even when its normal layout text differs.
#[test]
fn keypad_normal_application_and_kitty() {
    // Independent literal codes and SS3 finals come from kitty's functional
    // key table and xterm ctlseqs "Numeric Keypad", not encoder calculations.
    for (code, text, final_byte, kitty) in [
        ("Numpad0", "0", 'p', 57399),
        ("Numpad1", "1", 'q', 57400),
        ("Numpad2", "2", 'r', 57401),
        ("Numpad3", "3", 's', 57402),
        ("Numpad4", "4", 't', 57403),
        ("Numpad5", "5", 'u', 57404),
        ("Numpad6", "6", 'v', 57405),
        ("Numpad7", "7", 'w', 57406),
        ("Numpad8", "8", 'x', 57407),
        ("Numpad9", "9", 'y', 57408),
        ("NumpadDecimal", ".", 'n', 57409),
        ("NumpadDecimal", ",", 'n', 57409),
        ("NumpadDivide", "/", 'o', 57410),
        ("NumpadMultiply", "*", 'j', 57411),
        ("NumpadSubtract", "-", 'm', 57412),
        ("NumpadAdd", "+", 'k', 57413),
        ("NumpadEnter", "\r", 'M', 57414),
        ("NumpadEqual", "=", 'X', 57415),
    ] {
        let key = event(code, text, text, false);
        assert_eq!(
            encode_key(&key, InputModes::default()),
            text.as_bytes(),
            "{code}"
        );
        assert_eq!(
            encode_key(
                &key,
                InputModes {
                    application_keypad: true,
                    ..InputModes::default()
                }
            ),
            format!("\x1bO{final_byte}").as_bytes(),
            "{code}"
        );
        assert_eq!(
            encode_key(
                &key,
                InputModes {
                    kitty_flags: 8,
                    ..InputModes::default()
                }
            ),
            format!("\x1b[{kitty}u").as_bytes(),
            "{code}"
        );
    }
    located_keypad();
}

/// Location must retain keypad identity when a platform has no code.
fn located_keypad() {
    for (text, expected) in [("7", VtKey::Keypad(7)), (",", VtKey::KeypadOp('.'))] {
        let mut raw = RawKey::character(text, Some(text));
        raw.location = "Numpad".into();
        let Some(Command::Key(key)) = Adapter::default().apply(RawInput::Key(raw)) else {
            panic!("missing keypad");
        };
        assert_eq!(key.key, expected);
    }
}

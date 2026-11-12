//! Independent US keyboard rows, including every alphabet and keypad position.
use super::us_scalar;

/// Physical identity is US even when a layout remaps letters or punctuation.
#[test]
fn every_us_position() {
    for letter in 'A'..='Z' {
        assert_eq!(
            us_scalar(&format!("Key{letter}")),
            Some(letter.to_ascii_lowercase())
        );
    }
    for digit in '0'..='9' {
        for prefix in ["Digit", "Numpad"] {
            assert_eq!(us_scalar(&format!("{prefix}{digit}")), Some(digit));
        }
    }
    let codes = [
        "Backquote",
        "Minus",
        "Equal",
        "BracketLeft",
        "BracketRight",
        "Backslash",
        "Semicolon",
        "Quote",
        "Comma",
        "Period",
        "Slash",
        "Space",
        "NumpadAdd",
        "NumpadSubtract",
        "NumpadMultiply",
        "NumpadDivide",
        "NumpadDecimal",
        "NumpadEqual",
    ];
    for (code, scalar) in codes.into_iter().zip("`-=[]\\;',./ +-*/.=".chars()) {
        assert_eq!(us_scalar(code), Some(scalar), "{code}");
    }
    for invalid in [
        "KeyÄ",
        "Keyaa",
        "keyA",
        "Numpad10",
        "ArrowLeft",
        "Unidentified",
        "",
    ] {
        assert_eq!(us_scalar(invalid), None, "{invalid}");
    }
}

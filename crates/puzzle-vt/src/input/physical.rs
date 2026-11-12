//! W3C KeyboardEvent.code to unshifted US PC-101 scalars (§6.8, M7.2).
//!
//! This table describes positions, not the user's layout. Kitty's base-layout
//! alternate key must stay y for KeyY even when a German layout produces z.
//! Navigation and modifiers have no scalar and return None. Numpad identity
//! remains a typed key in the adapter; its scalar here is only a wire alternate.
//! Neither host may reconstruct base from committed text or a layout key.
//! IntlBackslash, IntlRo and IntlYen have no PC-101 US counterpart; returning
//! None omits an unknown alternate rather than inventing a shortcut identity.
//! Code spelling follows W3C UI Events code values (writing-system section).
//! https://www.w3.org/TR/uievents-code/#key-alphanumeric-writing-system

/// Literal nonalphabetic PC-101 positions, with no implicit layout transform.
const SYMBOLS: &[(&str, char)] = &[
    // The number row's primary characters do not depend on Shift state.
    ("Digit0", '0'),
    ("Digit1", '1'),
    ("Digit2", '2'),
    ("Digit3", '3'),
    ("Digit4", '4'),
    ("Digit5", '5'),
    ("Digit6", '6'),
    ("Digit7", '7'),
    ("Digit8", '8'),
    ("Digit9", '9'),
    // US punctuation positions can produce entirely different German characters.
    ("Backquote", '`'),
    ("Minus", '-'),
    ("Equal", '='),
    ("BracketLeft", '['),
    ("BracketRight", ']'),
    ("Backslash", '\\'),
    ("Semicolon", ';'),
    ("Quote", '\''),
    ("Comma", ','),
    ("Period", '.'),
    ("Slash", '/'),
    ("Space", ' '),
    // The numeric pad's physical decimal stays dot even on comma layouts.
    ("NumpadAdd", '+'),
    ("NumpadSubtract", '-'),
    ("NumpadMultiply", '*'),
    ("NumpadDivide", '/'),
    ("NumpadDecimal", '.'),
    ("NumpadEqual", '='),
];

/// Returns the US scalar of a physical code, never of a logical key name.
#[must_use]
pub fn us_scalar(code: &str) -> Option<char> {
    // Validate suffix lengths before converting bytes. Unknown Unicode input
    // cannot index outside a string or become an invented physical position.
    if let Some(letter) = code.strip_prefix("Key")
        && letter.len() == 1
        && letter.as_bytes()[0].is_ascii_uppercase()
    {
        return Some(char::from(letter.as_bytes()[0].to_ascii_lowercase()));
    }
    if let Some(digit) = code.strip_prefix("Numpad")
        && digit.len() == 1
        && digit.as_bytes()[0].is_ascii_digit()
    {
        return Some(char::from(digit.as_bytes()[0]));
    }
    // A small immutable punctuation table is shared across both shells;
    // it avoids a target-specific keyboard library in the pure VT crate.
    SYMBOLS
        .iter()
        .find_map(|(name, scalar)| (*name == code).then_some(*scalar))
}

#[cfg(test)]
/// Independent keyboard-row expectations protect every PC-101 scalar.
mod tests;

//! Validated plain-data boundary for physical keys (§5.7, §6.8).
//!
//! `KeyEvent` remains a real Rust payload on the bus. This module gives an
//! external recorder a stable map while rejecting unknown key identities
//! before they reach the input encoder. The tag string distinguishes a
//! printable scalar, an xterm function key, a keypad key and one of kitty's
//! published named PUA assignments. Numeric PUA gaps are rejected.
//!
//! The map also retains shifted and base identities. They matter on
//! international layouts, where a physical key and committed character
//! need not agree. The `text` field is optional committed input, not a
//! substitute for the key identity. Phase and modifier bits remain typed
//! after validation, so release events cannot become extra legacy presses.
//!
//! All map fields are explicit, including null optionals. That gives a
//! deterministic value→typed→value round trip for recordings and catches
//! a missing field instead of silently inventing a keyboard state.
//!
//! The enum-to-name mapping is exhaustive: adding a Rust `Key` variant
//! requires choosing a stable wire spelling. The inverse mapping checks
//! value ranges before constructing a variant. Function keys stop at 35
//! because kitty's extended table ends there; keypad digits stop at 9.
//! `NamedKey::from_code` consults the explicit published assignment list,
//! so a gap in the PUA block cannot be used to synthesize a host key.
//!
//! Numeric modifiers are represented as JSON-compatible numbers, but the
//! decoder requires a finite whole value in 0..255 before its u8 cast.
//! `NaN`, negative values and fractional bits are rejected with a report,
//! not clamped into a potentially different keyboard shortcut (R8).
//! Shifted and base fields each contain one Unicode scalar. Rejecting a
//! string of several scalars matters because the key encoder uses those
//! fields as alternate single-key identities, not as committed IME text.
//!
//! A key release can be harmless under legacy xterm and significant under
//! kitty. Preserving the phase even when the current mode ignores it means
//! replay under the same negotiated mode gives the same transport bytes.
//! The bus never needs this map while dispatching typed commands; allocation
//! occurs only when an external subscriber or recorder requests the wire.
use puzzle_bus::{Value, value::ValidationReport, wire::reject};
use puzzle_vt::input::{Key, KeyEvent, KeyPhase, Modifiers, NamedKey};
use std::collections::BTreeMap;

/// Converts one typed key into its stable wire map.
pub(super) fn to_value(event: &KeyEvent) -> Value {
    let mut map = BTreeMap::new();
    // The prefix prevents a private-use scalar from being confused with
    // a kitty functional key that happens to use the same integer code.
    map.insert("key".into(), Value::String(key_name(event.key)));
    // Modifiers are copied as an integer-shaped number because JS and JSON
    // share that representation without an unsigned-byte distinction.
    map.insert(
        "modifiers".into(),
        Value::Number(f64::from(event.modifiers.0)),
    );
    let phase = match event.phase {
        KeyPhase::Press => "press",
        KeyPhase::Repeat => "repeat",
        KeyPhase::Release => "release",
    };
    map.insert("phase".into(), Value::String(phase.into()));
    // Explicit nulls make absence different from a lost map field.
    map.insert("shifted".into(), optional_char(event.shifted));
    map.insert("base".into(), optional_char(event.base));
    map.insert(
        "text".into(),
        event
            .text
            .as_ref()
            .map_or(Value::Null, |text| Value::String(text.clone())),
    );
    // Older recordings omit these additions; omit absent metadata on output
    // too, preserving their value-to-typed-to-value identity (§5.7).
    if let Some(code) = &event.code {
        map.insert("code".into(), Value::String(code.clone()));
    }
    if event.composing {
        map.insert("composing".into(), Value::Bool(true));
    }
    Value::Map(map)
}

/// Builds a typed key only from the complete, bounded wire shape.
pub(super) fn from_value(value: &Value) -> Result<KeyEvent, ValidationReport> {
    // The tree-size gate in the caller has already bounded nesting and
    // string lengths, so these lookups can report shape errors precisely.
    let Value::Map(map) = value else {
        return Err(reject("key payload must be a map"));
    };
    let Some(Value::String(name)) = map.get("key") else {
        return Err(reject("key identity must be a string"));
    };
    let key = parse_key(name).ok_or_else(|| reject("unknown key identity"))?;
    // A key identity is not enough: forged modifiers could otherwise turn
    // a benign letter into an application shortcut.
    let Some(Value::Number(mods)) = map.get("modifiers") else {
        return Err(reject("key modifiers must be a number"));
    };
    // Kitty defines at most eight modifier bits in this neutral event.
    // Fractional, negative and NaN numbers cannot enter a bit field.
    if !mods.is_finite() || *mods < 0.0 || *mods > 255.0 || mods.fract() != 0.0 {
        return Err(reject("key modifiers are outside 0..255"));
    }
    let phase = match map.get("phase") {
        // Do not default to Press; a missing phase in a recording must
        // fail instead of injecting an extra key-down into the PTY.
        Some(Value::String(name)) if name == "press" => KeyPhase::Press,
        Some(Value::String(name)) if name == "repeat" => KeyPhase::Repeat,
        Some(Value::String(name)) if name == "release" => KeyPhase::Release,
        _ => return Err(reject("key phase must be press, repeat or release")),
    };
    let shifted = parse_optional_char(map.get("shifted"))?;
    let base = parse_optional_char(map.get("base"))?;
    let text = match map.get("text") {
        // Committed text may be multi-scalar (IME); the two alternate-key
        // fields above intentionally have stricter one-scalar semantics.
        Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text.clone()),
        _ => return Err(reject("key text must be string or null")),
    };
    let code = match map.get("code") {
        None => None,
        Some(Value::String(code)) if code.len() <= 64 => Some(code.clone()),
        _ => return Err(reject("physical code must be a bounded string")),
    };
    let composing = match map.get("composing") {
        None => false,
        Some(Value::Bool(value)) => *value,
        _ => return Err(reject("composing must be boolean")),
    };
    Ok(KeyEvent {
        key,
        code,
        composing,
        modifiers: Modifiers(*mods as u8),
        phase,
        shifted,
        base,
        text,
    })
}

/// Names every basic key family without relying on Rust Debug spelling.
fn key_name(key: Key) -> String {
    // Prefixes keep numeric families disjoint from human-readable
    // functional names while leaving common navigation keys legible.
    match key {
        Key::Char(ch) => format!("char:{}", u32::from(ch)),
        // Kitty's F13–F35 assignments stay functions, not PUA scalars.
        Key::Function(number) => format!("function:{number}"),
        // The keypad's application mode can encode these differently from
        // identical characters on the main keyboard.
        Key::Keypad(number) => format!("keypad:{number}"),
        Key::KeypadOp(ch) => format!("operator:{}", u32::from(ch)),
        Key::Named(named) => format!("named:{}", named.code()),
        // Navigation names remain stable even if Rust variants are renamed
        // for a platform adapter in a later milestone.
        Key::Up => "up".into(),
        Key::Down => "down".into(),
        Key::Right => "right".into(),
        Key::Left => "left".into(),
        Key::Home => "home".into(),
        Key::End => "end".into(),
        Key::Insert => "insert".into(),
        Key::Delete => "delete".into(),
        Key::PageUp => "pageUp".into(),
        Key::PageDown => "pageDown".into(),
        Key::Enter => "enter".into(),
        Key::KeypadEnter => "keypadEnter".into(),
        Key::Tab => "tab".into(),
        Key::Backspace => "backspace".into(),
        Key::Escape => "escape".into(),
    }
}

/// Decodes only valid key ranges; unknown strings never become text.
fn parse_key(name: &str) -> Option<Key> {
    // A scalar encoded numerically avoids delimiter escaping for a key
    // such as ':' or a non-ASCII layout character.
    if let Some(code) = name.strip_prefix("char:") {
        return char::from_u32(code.parse().ok()?).map(Key::Char);
    }
    if let Some(number) = name.strip_prefix("function:") {
        // Reject F0 and out-of-range F keys rather than passing them to
        // the legacy encoder as a silent no-op.
        let number = number.parse().ok()?;
        return (1..=35).contains(&number).then_some(Key::Function(number));
    }
    if let Some(number) = name.strip_prefix("keypad:") {
        // Numeric keypad values have one digit; other keypad operations
        // use their own operator family below.
        let number = number.parse().ok()?;
        return (0..=9).contains(&number).then_some(Key::Keypad(number));
    }
    if let Some(code) = name.strip_prefix("operator:") {
        // Only the keypad operators in §6.8 are meaningful in DECKPAM.
        let op = char::from_u32(code.parse().ok()?)?;
        return "+-*/.=".contains(op).then_some(Key::KeypadOp(op));
    }
    if let Some(code) = name.strip_prefix("named:") {
        // Explicit reverse lookup protects gaps in kitty's PUA table.
        return NamedKey::from_code(code.parse().ok()?).map(Key::Named);
    }
    Some(match name {
        // These names cover the M6 desktop keyboard set. Keypad and PUA
        // families were handled above so there is no ambiguous spelling.
        "up" => Key::Up,
        "down" => Key::Down,
        "right" => Key::Right,
        "left" => Key::Left,
        "home" => Key::Home,
        "end" => Key::End,
        "insert" => Key::Insert,
        "delete" => Key::Delete,
        "pageUp" => Key::PageUp,
        "pageDown" => Key::PageDown,
        "enter" => Key::Enter,
        "keypadEnter" => Key::KeypadEnter,
        "tab" => Key::Tab,
        "backspace" => Key::Backspace,
        "escape" => Key::Escape,
        _ => return None,
    })
}

/// Keeps optional scalar fields unambiguous across JSON and JS adapters.
fn optional_char(value: Option<char>) -> Value {
    // No empty-string sentinel: it could mean either absent or a malformed
    // scalar from a browser adapter.
    value.map_or(Value::Null, |ch| Value::String(ch.to_string()))
}

/// Accepts exactly one scalar or null, never a multi-scalar cluster.
fn parse_optional_char(value: Option<&Value>) -> Result<Option<char>, ValidationReport> {
    // `chars` yields Unicode scalars, not bytes, so an umlaut is one key
    // identity while a combining sequence is correctly rejected here.
    match value {
        Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => {
            let mut chars = text.chars();
            let Some(ch) = chars.next() else {
                return Err(reject("empty scalar field"));
            };
            if chars.next().is_some() {
                return Err(reject("scalar field has multiple characters"));
            }
            Ok(Some(ch))
        }
        _ => Err(reject("optional key scalar must be string or null")),
    }
}

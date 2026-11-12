//! Normalizes portable logical keys and exact physical codes (§14.9).
//!
//! Modifier aliases collapse before merge. Prefixes describe consumption,
//! not identity, so performable:cmd+c and super+c replace the same trigger.
//!
//! Two identities are deliberately kept separate: the character produced
//! by a layout and the physical position identified by W3C KeyboardEvent.code.
//! On German layouts KeyY produces z. A physical KeyY binding must win over
//! a logical z binding without changing the character sent to the program.
//! Physical codes retain exact casing; logical names accept the reference terminal's
//! case-insensitive spellings and remove separators only from named keys.
//! Literal punctuation takes the single-scalar path before normalization.
//! Without that ordering, a minus character would be erased as a separator.
//! The grammar uses plus, equals and greater as delimiters, so canonical
//! output spells those characters by name to remain parseable itself.
//! Canonicalization resolves modifier aliases and primary before comparing
//! layers. The prefix performable affects consumption, not chord identity.
//! A command modifier is represented by the same Super bit on every target;
//! primary alone is host-dependent and resolved with the supplied Platform.
//! Duplicate modifiers are errors: control+ctrl is usually an editing mistake,
//! not a meaningful way to demand two different physical Control keys.
//! Modifiers are exact bit sets. Adding Alt to a zoom gesture must stop it
//! matching, rather than intercepting a distinct shell or application chord.
//! The shifted-character exception is limited to logical character bindings.
//! US plus requires Shift, whereas German plus does not. Both match primary
//! plus without making Shift+Enter indistinguishable from Enter.
//! Explicit Shift in the binding is never discarded. A user can intentionally
//! bind Shift plus a character separately from its unshifted shortcut.
//! Named encoder keys map into DOM key names, not debug formatting of enums.
//! Function-key numbers are rendered dynamically so the same rule supports
//! F1 through F24 without inventing a second keyboard numbering scheme.
//! Physical function codes accept only positive published numbers; F0 is
//! not a keyboard key and a typo must not create an unreachable binding.
//! Letter and digit code suffixes are checked as ASCII before indexing.
//! That makes validation safe for arbitrary Unicode configuration strings.
//! The physical list contains US punctuation and international extra keys.
//! Those names represent positions rather than glyphs: BracketRight need not
//! produce a closing bracket on the user's active keyboard layout.
//! Keypad identity is preserved as a physical code even when NumLock changes
//! the logical key. Its encoder mode is a separate VT concern (§6.8).
//! The normalizer recognizes navigation names independent of spelling,
//! including arrow_up, arrow-up and ArrowUp as the same logical step.
//! Canonical physical names do not collapse into those logical names;
//! a user can keep separate physical and layout-dependent shortcuts.
//! Key::Named includes extended kitty identities which this grammar does
//! not yet name. Returning an empty name prevents accidental logical matches
//! while their physical code can still participate in a defined binding.
//! Only the lower-case ASCII modifier vocabulary is parsed as modifiers;
//! unknown words before a plus are rejected instead of becoming part of a key.
//! The final plus-separated component is always the key. Earlier components
//! are modifiers, so malformed empty keys are caught by logical validation.
//! Normalized identity is reused for configuration merges and CLI output.
//! Keeping one normalization routine prevents introspection describing a
//! shortcut that the actual matcher would resolve differently.
//! The test table includes exact modifier mismatches, US/German plus and
//! KeyY versus z, since each detects a different form of layout confusion.
//! The lookup returns owned strings during shortcut matching, a coarse input
//! path rather than the parser's per-byte or renderer's per-cell hot path.
//! There is no platform probe here: desktop provides winit data and web
//! provides DOM data, so both use exactly the same comparison contract.
//! Release handling and active composition are checked by matcher.rs first;
//! Trigger only answers whether a fully mapped key matches this chord.
//! Exact physical spellings are kept distinct from folded logical names.
//! This prevents a layout alias from silently changing a key position.
use super::Platform;
use crate::vt::input::{Key, KeyEvent, Modifiers};

/// One step in a leader sequence, independent of its action or layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Trigger {
    /// Kitty modifier bits, normalized from aliases and primary.
    pub modifiers: u8,
    /// Normalized logical name or exact W3C physical code.
    pub key: String,
    /// Physical identity takes precedence over a layout-aware binding.
    pub physical: bool,
}

/// Provides one identity contract for parsing, merging and matching.
impl Trigger {
    /// Validates a chord without guessing misspelled physical codes.
    pub fn parse(text: &str, platform: Platform) -> Result<Self, String> {
        let mut parts = text.split('+').collect::<Vec<_>>();
        let key = parts.pop().ok_or("missing key")?;
        let mut modifiers = 0;
        for part in parts {
            let bit = match part.to_ascii_lowercase().as_str() {
                "shift" => Modifiers::SHIFT,
                "ctrl" | "control" => Modifiers::CTRL,
                "alt" | "opt" | "option" => Modifiers::ALT,
                "super" | "cmd" | "command" => Modifiers::SUPER,
                "primary" => platform.primary(),
                _ => return Err(format!("unknown modifier: {part}")),
            };
            if modifiers & bit != 0 {
                return Err("duplicate modifier".into());
            }
            modifiers |= bit;
        }
        let physical = physical_code(key);
        let key = if physical { key.into() } else { logical(key)? };
        Ok(Self {
            modifiers,
            key,
            physical,
        })
    }

    /// Produces one stable identity for merge, duplicate detection and CLI.
    pub fn canonical(&self) -> String {
        let mut text = String::new();
        // This fixed order is independent of the spelling or order in config.
        for (bit, name) in [(4, "ctrl"), (2, "alt"), (1, "shift"), (8, "super")] {
            if self.modifiers & bit != 0 {
                text.push_str(name);
                text.push('+');
            }
        }
        let name = match self.key.as_str() {
            "+" => "plus",
            "-" => "minus",
            "=" => "equal",
            "," => "comma",
            "." => "period",
            "/" => "slash",
            " " => "space",
            ">" => "greater",
            key => key,
        };
        text.push_str(name);
        text
    }

    /// Matches exactly, except Shift used to produce a logical character.
    pub fn matches(&self, event: &KeyEvent) -> bool {
        if self.physical {
            return event.code.as_deref() == Some(self.key.as_str())
                && event.modifiers.0 == self.modifiers;
        }
        // Encoders keep the unshifted layout key. Logical shortcuts instead
        // compare the produced Shift character (US equal becomes plus).
        let produced = if event.modifiers.has(Modifiers::SHIFT) {
            event.shifted.map(Key::Char).unwrap_or(event.key)
        } else {
            event.key
        };
        let name = key_name(produced);
        let mut modifiers = event.modifiers.0;
        // Shift is ignored only for a produced character differing from its
        // base layout key, never for a named key such as Enter or an arrow.
        if let Key::Char(ch) = produced
            && self.modifiers & Modifiers::SHIFT == 0
            && event.shifted == Some(ch)
        {
            modifiers &= !Modifiers::SHIFT;
        }
        name == self.key && modifiers == self.modifiers
    }
}

/// W3C codes with their exact spelling; lowercase names remain logical.
pub(super) fn physical_code(key: &str) -> bool {
    let numbered = |prefix: &str, max: u8| {
        key.strip_prefix(prefix).is_some_and(|n| {
            n.parse::<u8>()
                .is_ok_and(|value| value <= max && value.to_string() == n)
        })
    };
    (key.len() == 4 && key.starts_with("Key") && key.as_bytes()[3].is_ascii_uppercase())
        || (key.len() == 6 && key.starts_with("Digit") && key.as_bytes()[5].is_ascii_digit())
        || numbered("Numpad", 9)
        || (numbered("F", 24) && key != "F0")
        || [
            "Backquote",
            "BracketLeft",
            "BracketRight",
            "Backslash",
            "IntlBackslash",
            "IntlRo",
            "IntlYen",
            "Comma",
            "Period",
            "Slash",
            "Semicolon",
            "Quote",
            "Minus",
            "Equal",
            "Space",
            "Enter",
            "Tab",
            "Escape",
            "Backspace",
            "ArrowUp",
            "ArrowDown",
            "ArrowLeft",
            "ArrowRight",
            "Home",
            "End",
            "Insert",
            "Delete",
            "PageUp",
            "PageDown",
            "NumpadAdd",
            "NumpadSubtract",
            "NumpadMultiply",
            "NumpadDivide",
            "NumpadDecimal",
            "NumpadEqual",
            "NumpadEnter",
            "NumpadComma",
        ]
        .contains(&key)
}

/// Logical aliases normalize names while retaining literal punctuation.
pub(super) fn logical(text: &str) -> Result<String, String> {
    if text.chars().count() == 1 {
        return Ok(text.to_lowercase());
    }
    let name = text.to_lowercase().replace(['_', '-'], "");
    let character = match name.as_str() {
        "plus" => Some("+"),
        "minus" => Some("-"),
        "equal" => Some("="),
        "comma" => Some(","),
        "period" => Some("."),
        "slash" => Some("/"),
        "space" => Some(" "),
        "greater" => Some(">"),
        _ => None,
    };
    if let Some(ch) = character {
        return Ok(ch.into());
    }
    if [
        "enter",
        "tab",
        "escape",
        "backspace",
        "arrowup",
        "arrowdown",
        "arrowleft",
        "arrowright",
        "pageup",
        "pagedown",
        "home",
        "end",
        "insert",
        "delete",
    ]
    .contains(&name.as_str())
        || name
            .strip_prefix('f')
            .is_some_and(|n| n.parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)))
    {
        Ok(name)
    } else {
        Err(format!("unknown key: {text}"))
    }
}

/// Maps typed encoder identities to W3C logical shortcut names.
pub(super) fn key_name(key: Key) -> String {
    match key {
        Key::Char(ch) => ch.to_lowercase().collect(),
        Key::Function(n) => format!("f{n}"),
        Key::Keypad(n) => n.to_string(),
        Key::KeypadOp(ch) => ch.to_string(),
        Key::Up => "arrowup".into(),
        Key::Down => "arrowdown".into(),
        Key::Left => "arrowleft".into(),
        Key::Right => "arrowright".into(),
        Key::Home => "home".into(),
        Key::End => "end".into(),
        Key::Insert => "insert".into(),
        Key::Delete => "delete".into(),
        Key::PageUp => "pageup".into(),
        Key::PageDown => "pagedown".into(),
        Key::Enter | Key::KeypadEnter => "enter".into(),
        Key::Tab => "tab".into(),
        Key::Backspace => "backspace".into(),
        Key::Escape => "escape".into(),
        Key::Named(_) => String::new(),
    }
}

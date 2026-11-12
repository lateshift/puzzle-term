//! DOM key identity and macOS Option policy before terminal encoding (§6.8).
//!
//! Ordinary text stays with textarea input, covering dead keys and mobile
//! keyboards. Kitty's US physical alternate is distinct from the user's
//! layout base. Chromium's getLayoutMap supplies exact layout characters.
//! Safari/Firefox fall back to the physical US table for Option-as-Alt,
//! as decided by the user on 2026-10-02. Default Option-as-text needs no map.
use puzzle_term::vt::input::{Key, KeyEvent, KeyPhase, Modifiers, physical::us_scalar};
use puzzle_term::{keybind::Platform, session::OptionAsAlt};
use std::collections::BTreeMap;
use web_sys::KeyboardEvent;
/// Named keys and numeric-pad identity independent of the produced text.
mod identity;

/// Side state survives keys whose location is Standard rather than Left/Right.
#[derive(Default)]
pub struct Keyboard {
    /// Left Option/Alt stays pressed until its keyup or blur.
    left_alt: bool,
    /// Right Option can remain a text modifier while left is terminal Alt.
    right_alt: bool,
    /// Layout characters, not kitty's physical-US alternate table.
    bases: BTreeMap<String, char>,
}

/// Host facts sampled per event, since apps can negotiate modes between keys.
#[derive(Clone, Copy)]
pub struct KeyPolicy {
    /// User OS chooses the default binding table, independent of wasm's host.
    pub platform: Platform,
    /// Which physical Option side is terminal Alt rather than layout text.
    pub option_as_alt: OptionAsAlt,
    /// Active-screen flags; only flag two requests releases.
    pub kitty_flags: u8,
    /// Browser chrome chords become available only with keyboard lock.
    pub locked: bool,
}

/// Mapping retains a native-text decision until keybindings are consulted.
pub struct MappedKey {
    /// Neutral metadata goes unchanged through the shared matcher/encoder.
    pub event: KeyEvent,
    /// Without a matching binding, textarea input owns this printable key.
    pub native_text: bool,
}

/// Keyboard state owns layout identity; it never writes application bytes.
/// Bases are supplied only by the optional browser layout API, never inferred
/// from preceding keystrokes: changing layouts cannot leave learned guesses.
/// The input handler consults keybindings before allowing native textarea
/// text, so an unmodified user binding can still consume a printable key.
impl Keyboard {
    /// Clears side state on blur because a release can reach another tab.
    pub fn reset(&mut self) {
        self.left_alt = false;
        self.right_alt = false;
    }

    /// Accepts a layout-map entry after checking that it is a single scalar.
    /// Unknown/dead-key strings cannot forge a numeric keypad or named key.
    pub fn set_layout_key(&mut self, code: &str, text: &str) {
        if let Some(ch) = scalar(text) {
            self.bases.insert(code.into(), ch);
        }
    }

    /// Maps a DOM press/repeat/release without taking committed text twice.
    /// None means a modifier, dead key, IME-owned key or unrequested release.
    /// The caller prevents default only for its accepted key path. A plain
    /// press stays native unless kitty's report-all flag or a binding owns it.
    /// Option text bypasses report-all because macOS treats Option as a
    /// layout modifier under the default policy, not a terminal Alt chord.
    pub fn map(&mut self, event: &KeyboardEvent, policy: KeyPolicy) -> Option<MappedKey> {
        let release = event.type_() == "keyup";
        match event.code().as_str() {
            "AltLeft" => self.left_alt = !release,
            "AltRight" => self.right_alt = !release,
            _ => {}
        }
        // IME's composing keys must not advance a pending leader sequence.
        if event.is_composing() || event.key() == "Dead" || (release && policy.kitty_flags & 2 == 0)
        {
            return None;
        }
        let (key, modifiers, option_text) = self.layout(event, policy)?;
        let mut mapped = KeyEvent::press(key);
        // A printable press can still match a physical or logical binding;
        // only the unbound path delegates its text to the input event.
        let native_text = matches!(key, Key::Char(_))
            && modifiers.0 & !Modifiers::SHIFT == 0
            && (policy.kitty_flags & 8 == 0 || option_text)
            && !release;
        mapped.phase = if release {
            KeyPhase::Release
        } else if event.repeat() {
            KeyPhase::Repeat
        } else {
            KeyPhase::Press
        };
        mapped.modifiers = modifiers;
        // W3C code identifies a position even when the active layout swaps
        // letters; kitty's base alternate must therefore come from code.
        mapped.code = Some(event.code());
        mapped.base = us_scalar(&event.code());
        mapped.shifted = event.shift_key().then(|| scalar(&event.key())).flatten();
        mapped.text = scalar(&event.key()).map(|ch| ch.to_string());
        // Option-as-Alt reports the unmodified layout key, not Option's text.
        if policy.platform == Platform::Mac
            && modifiers.has(Modifiers::ALT)
            && matches!(key, Key::Char(_))
        {
            mapped.text = None;
            mapped.shifted = None;
        }
        Some(MappedKey {
            event: mapped,
            native_text,
        })
    }

    /// Separates produced text, layout identity and the physical-US alternate.
    fn layout(
        &mut self,
        event: &KeyboardEvent,
        policy: KeyPolicy,
    ) -> Option<(Key, Modifiers, bool)> {
        let produced = scalar(&event.key());
        let mut modifiers = modifiers(
            event.shift_key(),
            event.alt_key(),
            event.ctrl_key(),
            event.meta_key(),
        );
        let option_text = policy.platform == Platform::Mac && event.alt_key() && produced.is_some();
        let mut key = identity::key(&event.key(), &event.code(), event.location())?;
        if option_text
            && !policy
                .option_as_alt
                .treats_as_alt(self.left_alt, self.right_alt)
        {
            // Clear Alt before deciding between bindings and native text.
            modifiers.0 &= !Modifiers::ALT;
        } else if option_text {
            // The documented non-Chromium fallback is deterministic and never
            // drops an Alt chord merely because getLayoutMap is unavailable.
            let base = self
                .bases
                .get(&event.code())
                .copied()
                .or_else(|| us_scalar(&event.code()))
                .or(produced)?;
            key = Key::Char(base);
        } else if event.shift_key() && matches!(key, Key::Char(_)) {
            // getLayoutMap preserves punctuation bases; a one-scalar lower
            // case fallback is safe for letters, never a physical US guess.
            let lower = scalar(&event.key().to_lowercase());
            if let Some(base) = self.bases.get(&event.code()).copied().or(lower) {
                key = Key::Char(base);
            }
        }
        Some((key, modifiers, option_text))
    }
}

/// Booleans use the shared modifier bit layout, not CSI's one-based number.
pub(super) fn modifiers(shift: bool, alt: bool, ctrl: bool, meta: bool) -> Modifiers {
    Modifiers(u8::from(shift) | (u8::from(alt) * 2) | (u8::from(ctrl) * 4) | (u8::from(meta) * 8))
}

/// A W3C named value is not a committed Unicode scalar.
pub(super) fn scalar(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    chars.next().is_none().then_some(ch)
}

/// Leaves browser chrome shortcuts native unless fullscreen lock was acquired.
pub fn reserved(key: Key, modifiers: Modifiers, platform: Platform) -> bool {
    modifiers.0 == platform.primary() && matches!(key, Key::Char('w' | 't' | 'n' | 'q'))
}

#[cfg(test)]
/// Synthetic browser keys cross the same layout mapper as real DOM input.
mod tests;

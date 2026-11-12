//! Recorded macOS callback shapes for all K4 simulator chords.

use super::{Source, for_chord};
use crate::keyboard::raw::{RawInput, RawLogical};

/// A, D, U, X and Shift+D retain their recorded key identity and order.
#[test]
fn recorded_key_fields_and_modifiers() {
    for (chord, letter, bits, entry) in [
        ("ctrl+a", "a", 4, 1016),
        ("ctrl+d", "d", 4, 212),
        ("ctrl+u", "u", 4, 281),
        ("ctrl+x", "x", 4, 469),
        ("ctrl+shift+d", "D", 5, 661),
    ] {
        let Ok((events, source)) = for_chord(chord) else {
            panic!("missing {chord}");
        };
        assert_eq!(source, Source::Recorded(entry));
        assert_eq!(events.first(), Some(&RawInput::Modifiers(bits)));
        assert_eq!(events.last(), Some(&RawInput::Modifiers(0)));
        let (Some(RawInput::Key(press)), Some(RawInput::Key(release))) =
            (events.get(1), events.get(2))
        else {
            panic!("missing transitions");
        };
        assert_eq!(press.logical, RawLogical::Character(letter.into()));
        assert_eq!(press.text.as_deref(), Some(letter));
        assert!(press.pressed);
        assert!(!release.pressed);
        assert_eq!(release.text, None);
    }
}

/// The six unrecorded letters remain explicitly marked as inferred.
#[test]
fn unrecorded_letters_are_not_claimed_as_observed() {
    for letter in ['e', 'g', 'p', 'q', 'r', 't'] {
        let chord = format!("ctrl+{letter}");
        assert!(matches!(for_chord(&chord), Ok((_, Source::Inferred))));
    }
    for letter in 'a'..='z' {
        assert!(for_chord(&format!("ctrl+{letter}")).is_ok());
    }
}

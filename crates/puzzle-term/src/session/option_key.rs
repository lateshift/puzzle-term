//! Host-neutral preference for interpreting the macOS Option key (§6.8).
//!
//! A keyboard layout may use Option to produce ordinary text. The desktop
//! adapter needs the preference before it chooses text or an Alt key report;
//! keeping the value in SessionConfig lets settings change the policy later.

/// Which physical Option keys act as terminal Alt rather than layout keys.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OptionAsAlt {
    /// Both Option keys remain text modifiers, the macOS convention.
    #[default]
    None,
    /// Only the left Option key produces terminal Alt chords.
    Left,
    /// Only the right Option key produces terminal Alt chords.
    Right,
    /// Both Option keys produce terminal Alt chords.
    Both,
}

impl OptionAsAlt {
    /// Tests the physical side without depending on a platform event type.
    #[must_use]
    pub fn treats_as_alt(self, left: bool, right: bool) -> bool {
        match self {
            Self::None => false,
            Self::Left => left,
            Self::Right => right,
            Self::Both => left || right,
        }
    }
}

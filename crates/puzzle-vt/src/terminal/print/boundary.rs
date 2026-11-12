//! Incremental extended grapheme boundaries, Unicode 17 UAX #29 revision 47.
//!
//! A scalar's property and a fixed-size suffix state decide GB3–GB999 without
//! constructing UTF-8 candidates or rescanning the preceding cluster. GB9c,
//! GB11 and GB12/13 need conjunct, emoji and RI suffixes, respectively. The
//! property pages are generated from our already-approved segmentation crate;
//! regenerate with boundary/generate.py and its 1.13.3 source directory.
//! This matches that dependency's Unicode version rather than upgrading rules.

/// Disjoint category IDs match the pinned generator's explicitly ordered list.
mod category;
use category::*;

/// Low four bits are the GCB/EP/Indic-consonant category; upper bits are InCB.
#[derive(Clone, Copy, Debug, Default)]
pub(in crate::terminal) struct Property(u8);

impl Property {
    /// Looks up one scalar through a deduplicated 256-scalar property page.
    #[inline]
    pub(super) fn of(scalar: char) -> Self {
        Self::of_codepoint(scalar as u32)
    }

    /// Also accepts an externally edited cell scalar without a UTF-8 conversion.
    #[inline]
    pub(super) fn of_codepoint(scalar: u32) -> Self {
        /// 4,352 byte page indices, then unique 256-byte property pages.
        const TABLE: &[u8] = include_bytes!("boundary/properties.bin");
        // Invalid cells behave like the old U+FFFD fallback. Surrogate slots
        // also have Other properties, so no char conversion is needed here.
        let scalar = if scalar < 0x110000 {
            scalar as usize
        } else {
            0xfffd
        };
        let page = TABLE[scalar >> 8];
        // Uniform pages encode their property directly in the low seven bits.
        // Nonuniform pages use the high bit plus a deduplicated page number.
        if page < 128 {
            return Self(page);
        }
        Self(TABLE[4352 + (page as usize - 128) * 256 + (scalar & 255)])
    }

    /// Returns the disjoint category used by adjacent-pair rules.
    #[inline]
    fn category(self) -> u8 {
        self.0 & 15
    }

    /// Decides a pair whose previous cell contains exactly one scalar.
    #[inline]
    pub(super) fn joins_single(self, next: Self) -> bool {
        // A single RI has odd parity. Emoji/conjunct contextual actions cannot
        // succeed from a lone scalar: both require a preceding suffix sequence.
        let action = PAIRS[self.category() as usize * 16 + next.category() as usize];
        action == 1 || action == 4
    }

    /// Preserves the previous GB11 width probe's joining result (D-31).
    pub(super) fn presentation_candidate(self) -> bool {
        // The old EP+ZWJ probe also accepted GB9a spacing marks. Preserve
        // that behavior here; actual grapheme boundaries use the full state.
        matches!(self.category(), PICTOGRAPH | EXTEND | ZWJ | SPACING)
    }
}

/// Bounded suffix context for one cluster; no heap or byte/string ownership.
#[derive(Clone, Copy, Debug)]
pub(super) struct State {
    /// Immediately preceding scalar's category for GB3–9b.
    previous: u8,
    /// Odd-length consecutive RI suffix for GB12/13.
    odd_ri: bool,
    /// Suffix is EP Extend*, permitting a following ZWJ to arm GB11.
    emoji_extend: bool,
    /// Suffix is EP Extend* ZWJ, permitting a following EP to join.
    emoji_zwj: bool,
    /// 0: none, 1: consonant with no linker, 2: consonant with a linker.
    conjunct: u8,
    /// Legacy grid extras may contain multiple clusters; they cannot be extended.
    single: bool,
}

impl State {
    /// Seeds suffixes from a cell's first scalar, without creating cluster text.
    #[inline]
    pub(super) fn start(property: Property) -> Self {
        let category = property.category();
        Self {
            previous: category,
            odd_ri: category == RI,
            emoji_extend: category == PICTOGRAPH,
            emoji_zwj: false,
            conjunct: u8::from(category == CONSONANT),
            single: true,
        }
    }

    /// Recovers context after cursor movement, edits or legacy-mode cluster writes.
    pub(super) fn from_text(text: &str) -> Option<Self> {
        let mut chars = text.chars();
        let mut state = Self::start(Property::of(chars.next()?));
        // This rare recovery borrows authoritative text; ordinary sequential
        // joins use the cached state and perform no scan or string construction.
        for scalar in chars {
            state.push(Property::of(scalar));
        }
        Some(state)
    }

    /// Applies the ordered UAX rules to the next scalar's property.
    #[inline]
    pub(super) fn joins(self, property: Property) -> bool {
        if !self.single {
            return false;
        }
        let after = property.category();
        // Most pairs break immediately. One tiny immutable transition table
        // selects the rare contextual rule instead of branching through every
        // Hangul, control and extension rule for every independent scalar.
        match PAIRS[self.previous as usize * 16 + after as usize] {
            1 => true,
            2 => self.conjunct == 2,
            3 => self.emoji_zwj,
            4 => self.odd_ri,
            _ => false,
        }
    }

    /// Advances suffixes exactly once, retaining whether the cell is one cluster.
    pub(super) fn push(&mut self, property: Property) {
        let category = property.category();
        self.single &= self.joins(property);
        // GB11 ignores only GCB Extend; a second ZWJ disarms the emoji suffix.
        self.emoji_zwj = category == ZWJ && self.emoji_extend;
        self.emoji_extend = category == PICTOGRAPH || (category == EXTEND && self.emoji_extend);
        // RI parity resets on any non-RI, including an attached Extend scalar.
        self.odd_ri = category == RI && (self.previous != RI || !self.odd_ri);
        // A linker counts only after a consonant, and an InCB Extend preserves
        // that suffix. Other categories terminate it even if GB9a attaches them.
        self.conjunct = if category == CONSONANT {
            1
        } else if property.0 & 32 != 0 && self.conjunct != 0 {
            2
        } else if property.0 & 16 != 0 {
            self.conjunct
        } else {
            0
        };
        self.previous = category;
    }
}

#[cfg(test)]
mod tests;

/// Pair actions: break, join, conjunct, emoji-ZWJ or regional-indicator parity.
const PAIRS: [u8; 256] = pair_actions();

/// Generates the ordered UAX #29 pair rules at compile time, never during parsing.
const fn pair_actions() -> [u8; 256] {
    let mut pairs = [0; 256];
    let mut index = 0;
    while index < pairs.len() {
        let before = (index / 16) as u8;
        let after = (index % 16) as u8;
        pairs[index] = match (before, after) {
            (CR, LF) => 1,                                               // GB3
            (CR | LF | CONTROL, _) | (_, CR | LF | CONTROL) => 0,        // GB4/5
            (L, L | V | LV | LVT) | (LV | V, V | T) | (LVT | T, T) => 1, // GB6–8
            (_, EXTEND | ZWJ | SPACING) | (PREPEND, _) => 1,             // GB9/9a/9b
            (_, CONSONANT) => 2,                                         // GB9c
            (ZWJ, PICTOGRAPH) => 3,                                      // GB11
            (RI, RI) => 4,                                               // GB12/13
            _ => 0,                                                      // GB999
        };
        index += 1;
    }
    pairs
}

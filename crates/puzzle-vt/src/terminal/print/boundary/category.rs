//! Category encoding shared with the Unicode 17 property-page generator.
//! These IDs combine mutually exclusive GCB classes with two GB9c/11 classes
//! whose GCB value is Other. Separate InCB Extend/Linker bits stay in Property.

/// Carriage return, joined only to LF before other control rules (GB3).
pub(super) const CR: u8 = 1;
/// Controls break on either side (GB4/5).
pub(super) const CONTROL: u8 = 2;
/// Combining extension; also preserves an emoji prefix (GB9/11).
pub(super) const EXTEND: u8 = 3;
/// Extended_Pictographic, a GCB Other scalar in the pinned table (GB11).
pub(super) const PICTOGRAPH: u8 = 4;
/// Indic conjunct consonant, a GCB Other scalar in the pinned table (GB9c).
pub(super) const CONSONANT: u8 = 5;
/// Hangul leading jamo (GB6).
pub(super) const L: u8 = 6;
/// Line feed, joined only after CR (GB3).
pub(super) const LF: u8 = 7;
/// Hangul syllable with no trailing jamo (GB7).
pub(super) const LV: u8 = 8;
/// Hangul syllable with trailing jamo (GB8).
pub(super) const LVT: u8 = 9;
/// Prepending scalar, joined to the following non-control (GB9b).
pub(super) const PREPEND: u8 = 10;
/// Regional indicator; pairs join by consecutive-suffix parity (GB12/13).
pub(super) const RI: u8 = 11;
/// Spacing combining mark, attached under the extended rules (GB9a).
pub(super) const SPACING: u8 = 12;
/// Hangul trailing jamo (GB8).
pub(super) const T: u8 = 13;
/// Hangul vowel jamo (GB7).
pub(super) const V: u8 = 14;
/// Zero width joiner, attached by GB9 and potentially arming GB11.
pub(super) const ZWJ: u8 = 15;

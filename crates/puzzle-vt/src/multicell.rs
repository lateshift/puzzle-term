//! Sparse OSC 66 blocks preserve the sixteen-byte ordinary cell (M18.7).
//!
//! A screen ring owns the store; row handles carry only references to their
//! fragments. Retiring a row releases just those references, never scans history.
//! Slot reuse happens only after the last fragment is gone. Sealed publications
//! later copy values rather than sharing these mutable slot identities (D-50).
use crate::LineId;
use smallvec::SmallVec;

/// Free-list ownership and checked, nonwrapping debug generations.
mod store;
pub use store::MulticellStore;

/// Kitty text sizing metadata; fractional scale changes ink, never occupancy.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TextSizing {
    /// Integer occupied height, restricted to 1..=7 by the wire parser.
    pub scale: u8,
    /// Explicit width in scaled cells; zero uses D-31 grapheme widths.
    pub width: u8,
    /// Fraction numerator, zero disables fractional rendering.
    pub numerator: u8,
    /// Fraction denominator; active only when greater than the numerator.
    pub denominator: u8,
    /// Vertical alignment: top, bottom, centered.
    pub vertical: u8,
    /// Horizontal alignment: left, right, centered.
    pub horizontal: u8,
}
impl Default for TextSizing {
    /// Default sizing occupies the same cells as ordinary text.
    fn default() -> Self {
        Self {
            scale: 1,
            width: 0,
            numerator: 0,
            denominator: 0,
            vertical: 0,
            horizontal: 0,
        }
    }
}

/// One retained physical row of a rectangular block.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Fragment {
    /// Stable row identity; row rotation does not rewrite this value.
    pub line: LineId,
    /// Original vertical offset, retained even after the top enters history.
    pub offset: u8,
}

/// Immutable-to-read block value; only the owning VT may change it.
#[derive(Clone, Debug)]
pub struct Multicell {
    /// Exact logical payload, emitted once rather than once per covered cell.
    pub text: String,
    /// Original protocol sizing and alignment.
    pub sizing: TextSizing,
    /// Left edge shared by all retained fragments.
    pub column: usize,
    /// Occupied physical width, independent of variation selectors and font size.
    pub columns: usize,
    /// At most seven rows, stored inline without a separate heap allocation.
    pub fragments: SmallVec<[Fragment; 7]>,
    /// Mutation revision for render-side block caches.
    pub revision: u64,
}

/// Row-side witness; generation exists only in debug/test builds.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct BlockRef {
    /// Cell.ch stores this same slot index whenever MULTICELL is set.
    pub key: u32,
    /// Debug generations detect an accidentally retained reference after reuse.
    #[cfg(debug_assertions)]
    pub(crate) generation: u64,
}

//! Scalar facts and the last joined suffix are rebuildable print-side caches.
//! A property is reusable by scalar value; a suffix additionally checks row
//! identity/version and screen epoch. Keeping these together makes their
//! construction lifetime explicit without changing either boundary algorithm.
use super::{Property, Tail};

/// One cached property and one validated suffix, independent of cluster storage.
pub(in crate::terminal) struct GraphemeCache {
    /// Avoids looking up the preceding scalar's Unicode property twice.
    pub(in crate::terminal) property: (u32, Property),
    /// A row/version witness prevents reuse after a cursor move or edit.
    pub(in crate::terminal) tail: Option<Tail>,
    /// Differential tests select the accepted pre-incremental oracle only here.
    #[cfg(test)]
    pub(in crate::terminal) reference: bool,
}

/// An unwritten NUL must not accidentally match an initialized property fact.
impl Default for GraphemeCache {
    fn default() -> Self {
        Self {
            property: (u32::MAX, Property::default()),
            tail: None,
            #[cfg(test)]
            reference: false,
        }
    }
}

//! Frozen hover data from the Session owner and its presented row versions.
//!
//! Cells name text, not glyph handles. Grouped OSC links can occupy several
//! disjoint rows. The drawing half resolves those cells through fresh geometry;
//! it never scans history, detects URLs or changes the opening policy.
//! A stale presented row is excluded before its target reaches this source.
//! The target remains displayable even when its scheme is disallowed.
use crate::geometry::Cell;

/// One validated hover target, with reusable string and cell allocations.
#[derive(Default, Debug, PartialEq)]
pub struct Link {
    /// URI is data for a tooltip; only the Session may request an opener.
    pub uri: String,
    /// Every visible cell of the explicit group or detected logical-line URL.
    pub cells: Vec<Cell>,
    /// Platform primary modifier, supplied by the host's binding convention.
    pub primary: u8,
    /// Scheme and configured gesture policy have both accepted this target.
    pub openable: bool,
}
/// Rotating snapshots retain each slot's warmed storage rather than allocating.
impl Clone for Link {
    /// Cold publication creates independent ownership of untrusted text.
    fn clone(&self) -> Self {
        let mut out = Self::default();
        out.clone_from(self);
        out
    }
    /// Source updates reuse capacity and cannot mutate an earlier sealed value.
    fn clone_from(&mut self, source: &Self) {
        self.uri.clone_from(&source.uri);
        self.cells.clone_from(&source.cells);
        self.primary = source.primary;
        self.openable = source.openable;
    }
}

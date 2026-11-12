//! View-local native drop messages become bounded protocol values on Session.
//! Coordinates include padding, just like SurfacePointer. Conversion uses the
//! last presented geometry; no host caches a grid origin or subtracts it twice.
use puzzle_vt::clipboard::MimeData;
pub use puzzle_vt::dnd::{Effect, MAX_BYTES, MAX_ITEMS};
/// Native gesture phase; payload bytes are allowed only for an actual drop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    /// Offer names without reading or retaining their data.
    Move,
    /// Release every resource when the drag leaves the view.
    Leave,
    /// A user gesture grants access to the supplied immutable representations.
    Drop,
}
/// One native observation, returned to the registration's creating surface only.
#[derive(Clone, Debug)]
pub struct Input {
    /// Echo the current drop/registration callback's generation, never another surface's.
    pub generation: u64,
    /// Move, leave or completed drop.
    pub kind: Kind,
    /// View-local logical points with top-left origin, including padding.
    pub position: [f64; 2],
    /// Allowed copy/move bitmask; ignored for Leave.
    pub operations: u32,
    /// Ordered MIME representations, empty data on Move, absent on Leave.
    pub items: Vec<MimeData>,
}

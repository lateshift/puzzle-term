//! Anchor resolution reuses VT's tracked-position registry (design §6.3, §11.1).
//!
//! The Session owner seals resolved handles in its publication; views read these
//! small resolved records. An offscreen anchor is hidden, not deleted. Gone
//! handles are distinct and can remove their declarative content permanently.
//!
//! Cell/range/line anchors contain generation-checked VT handles. An old
//! handle can never attach to a replacement slot after eviction or release.
//! Resolving into plain coordinates lets a frame survive parser activity
//! after capture without consulting the live registry again.
//!
//! Range endpoints are inclusive, matching selection and search. The first
//! endpoint supplies the relative origin; range geometry supplies row fills.
//! A row anchor tracks column zero so reflow uses VT's authoritative mapping.
//! Cursor and pointer anchors instead sample transient presentation state.
//! A hidden caret must not create a clickable control at its stale position.
//!
//! Zone identities come from OSC 133, not an application's numeric input.
//! Evicted zone marks use a sentinel line and are treated as Gone. Viewport
//! anchors remain meaningful when no grid text is visible and can draw Top
//! content into padding. Offscreen cell anchors retain their declarations;
//! returning to that content reveals the same overlay without rebuilding it.
use crate::geometry::{Cell, Range, Rect, TextGeometry};
use puzzle_vt::{
    terminal::Terminal,
    tracked::{LineId, PositionState, TrackedPos},
    zones::ZoneKind,
};

/// Opaque tracked endpoint supplied by Session::add_marker.
pub type Marker = TrackedPos;
/// Two tracked cells delimiting an inclusive interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkerRange {
    /// Beginning follows scroll, insertion and reflow.
    pub start: Marker,
    /// End follows the same authoritative VT update rules.
    pub end: Marker,
}
/// Location to resolve afresh for each frame, before anchor-relative drawing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    /// A cell tracked by the terminal registry.
    Cell(Marker),
    /// A tracked inclusive cell interval.
    Range(MarkerRange),
    /// A row tracked through reflow by a marker at column zero.
    Line(Marker),
    /// OSC 133 output block identified by its tracked start mark.
    Zone(u64),
    /// The current application caret.
    Cursor,
    /// Fixed logical rectangle on the surface.
    Viewport(Rect),
    /// The most recent logical pointer position.
    Pointer,
}
/// A sampled anchor independent of the live VT borrow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Resolved {
    /// Grid interval; geometry will test whether it is currently visible.
    Cells(Range),
    /// Fixed logical rectangle supplied by the caller.
    Rect(Rect),
    /// No current surface location (for example no pointer has entered).
    Hidden,
    /// Referenced text was evicted or the handle was released.
    Gone,
}
/// Live VT sampling is separate from logical-space frame placement.
impl Anchor {
    /// Samples live positions while the host still owns its short Session borrow.
    pub fn sample(self, terminal: &Terminal, pointer: Option<[f32; 2]>) -> Resolved {
        let cell = |marker| match terminal.tracked.get(marker) {
            PositionState::Present { line, column } => Some(Cell { line, column }),
            PositionState::Gone => None,
        };
        match self {
            Self::Cell(marker) | Self::Line(marker) => cell(marker).map_or(Resolved::Gone, |pos| {
                Resolved::Cells(Range {
                    start: pos,
                    end: pos,
                })
            }),
            Self::Range(markers) => match (cell(markers.start), cell(markers.end)) {
                (Some(start), Some(end)) => Resolved::Cells(Range { start, end }),
                _ => Resolved::Gone,
            },
            Self::Zone(id) => terminal
                .zones
                .iter()
                .find(|mark| {
                    mark.kind == ZoneKind::Output && mark.id == id && mark.line != LineId(u64::MAX)
                })
                .map_or(Resolved::Gone, |mark| {
                    let pos = Cell {
                        line: mark.line,
                        column: mark.column,
                    };
                    Resolved::Cells(Range {
                        start: pos,
                        end: pos,
                    })
                }),
            Self::Cursor => terminal
                .buffer()
                .cursor()
                .filter(|cursor| cursor.visible)
                .map_or(Resolved::Hidden, |cursor| {
                    let pos = Cell {
                        line: cursor.line,
                        column: cursor.column,
                    };
                    Resolved::Cells(Range {
                        start: pos,
                        end: pos,
                    })
                }),
            Self::Viewport(rect) => Resolved::Rect(rect),
            Self::Pointer => pointer.map_or(Resolved::Hidden, |origin| {
                Resolved::Rect(Rect {
                    origin,
                    size: [0.0; 2],
                })
            }),
        }
    }
}
/// Frozen attachments can be located without a live parser borrow.
impl Resolved {
    /// Finds a current drawing origin, preserving the hidden/Gone distinction.
    pub fn origin(self, geometry: &TextGeometry) -> Option<[f32; 2]> {
        match self {
            Self::Cells(range) => geometry.text_rect(range.start).map(|rect| rect.origin),
            Self::Rect(rect) => Some(rect.origin),
            Self::Hidden | Self::Gone => None,
        }
    }
}
/// Converts a raw line identity to a tracked row anchor through one registry.
pub fn track_line(terminal: &mut Terminal, line: LineId) -> Anchor {
    Anchor::Line(terminal.tracked.track(line, 0))
}

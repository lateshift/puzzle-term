//! Pure terminal state built from reusable fixed-width rows (design §6).
//!
//! The parser, grid and scrollback live here so platform shells can feed
//! identical byte streams without supplying a clock, font or I/O handle.
//! Screen rows retain allocations as they rotate through the scrollback ring.
/// Incremental, bounded kitty graphics APC extraction before the VT parser.
pub use protocols::kitty::apc;
/// A sixteen-byte cell with inline style and flags.
pub mod cell;
/// Shared X11 colour-spec decoder for every colour sequence.
pub mod color_spec;
/// Unified theme defaults and application overrides.
pub mod color_table;
/// Dirty-row and scroll information consumed by a future frame builder.
pub mod damage;
/// Pure keyboard, mouse, focus and paste encoders for the host transport.
pub mod input;
/// Bounded hyperlink targets with stale-handle rejection.
pub mod links;
/// Fixed supported-mode table for DECSET and DECRQM.
pub mod modes;
/// Bounded vte callbacks and sequence-family dispatch.
mod perform;
/// Extension handlers grouped by the project that defined their wire format.
mod protocols;
/// Recycled row storage with independent line and byte limits.
pub mod ring;
/// A full-width row and its sparse, column-sorted supplementary data.
pub mod row;
/// Primary or alternate screen state built over rows.
pub mod screen;
/// Incremental literal search over logical soft-wrapped lines.
pub mod search;
/// Tracked simple, word, line, block and shell-semantic selections.
pub mod selection;
/// Owns the active screen and cursor, without platform facilities.
pub mod terminal;
/// A stable line handle and registry for positions in retained text.
pub mod tracked;
/// Borrowed contents API for cells, logical lines and serialization.
pub mod view;
/// Bounded OSC 133 shell prompt and command marks.
pub mod zones;

pub use cell::{Attrs, Cell, CellFlags, PackedColor};
pub use row::{CellExtra, Row, RowFlags};
pub use screen::Screen;
pub use terminal::{Cursor, Terminal};
pub use tracked::{LineId, PrivateTrackedPos, TrackedPos, TrackedPositions};
pub use view::BufferView;

/// Test-only operation observer; absent from every production target.
#[cfg(test)]
#[path = "ring/tests/operations.rs"]
pub(crate) mod scroll_operations;

#[cfg(test)]
/// Shared functional-test boundary; production feeds never invoke a full scan.
mod test_grid;

/// Session extension capture values keep protocol-specific state outside VT.
pub mod osc;

/// Bounded clipboard transaction values shared by wire handlers and hosts.
pub mod clipboard;

/// Completed desktop notification values and shared security bounds.
pub mod notification;

/// Lazy drag/drop state and typed native observations.
pub mod dnd;

/// Bounded execution-context values for contents queries and host facts.
pub mod context;

/// Sparse rectangular grid text for kitty OSC 66.
pub mod multicell;

/// Monotonic content versions, separate from physical row identities.
mod write_clock;
pub use write_clock::{RangeRelation, RangeWatch, WatchCounts, WatchState, WriteClock};

/// Lazy reply packet ownership for completion-accounted Session transports.
pub mod reply_stream;

/// Operation attribution for unused scalar paths, absent from production builds.
#[cfg(test)]
#[path = "terminal/tests/print_work.rs"]
pub(crate) mod print_work;

/// Byte bound for shell command labels exposed to embedders (M12.tabs G1).
pub use protocols::kitty::shell::MAX_COMMAND_BYTES;

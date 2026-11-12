//! Puzzle Terminal's checked C interface over puzzle-embed (D-43).
//!
//! App and Surface IDs include namespace and generation; they are not pointers.
//! All host calls run on the creating thread, with worker-safe wakeup as the
//! sole callback exception. See include/puzzle.h for native object lifetime and
//! borrowed buffer contracts. Terminal behavior lives entirely in puzzle-embed.
#![deny(
    clippy::undocumented_unsafe_blocks,
    clippy::multiple_unsafe_ops_per_block
)]
/// Stable C data layouts and error codes; numeric inputs are validated explicitly.
pub mod abi;
/// Generation-checked capability storage, covered by pure Miri tests.
mod handles;
/// Sole input/output pointer marshalling helpers.
mod memory;
/// Native window handles are borrowed under the host's explicit lifetime contract.
mod native;
/// Checked App affinity and callback delivery lifetimes.
mod registry;

/// Host lifecycle and surface registration exports.
mod app;
pub use app::*;
/// Native MIME clipboard requests complete asynchronously through owner messages.
mod clipboard;
/// Owned events become call-scoped C callback buffers.
mod events;
/// Native notification effects and asynchronous lifecycle feedback.
mod notification;
pub use clipboard::*;
pub use notification::*;

/// Native key, pointer, text and composition exports.
mod input;
pub use input::*;

/// Geometry, settings, search, transport and builtin-overlay exports.
mod surface;
pub use surface::*;
/// Replaceable native find UI and nonblocking selection text queries.
mod search;
pub use search::*;
/// Host-native UI covers movable overlays without changing terminal padding.
mod occlusion;
pub use occlusion::*;
/// Physical host refresh delivery shares the render owner's native pacing policy.
mod refresh;
pub use refresh::*;

/// Tracked markers and every M9 declarative annotation kind.
mod decorations;
pub use decorations::*;

#[cfg(test)]
/// Pure marshalling runs in Miri; native lifecycle additionally runs under ASan.
mod tests;

/// Native drag observations and registration/acceptance callback translation.
mod dnd;
pub use dnd::*;

// Extra protocols can be removed without rewriting the base interface.
include!("optional_items.rs");

/// Optional host clocks do not change existing ABI-v1 input layouts.
mod input_timing;
pub use input_timing::*;

/// Hosts acknowledge unread output without waiting for a worker.
mod activity;
pub use activity::*;
/// Portable host-owned thumbnails of the last presented native image.
mod snapshot;
pub use snapshot::*;

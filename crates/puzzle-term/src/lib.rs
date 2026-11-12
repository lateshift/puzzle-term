//! Session composition will be added in M6 (design §4.5).
//!
//! Session composition will join the leaf crates while leaving GPU ownership to the shells.
/// Shared pure binary codec for the development PTY transport.
pub mod bridge;
/// Stable warning and error event names for the composition layer.
pub mod diagnostics;
/// Damage-driven terminal rows translated into neutral GPU instances.
pub mod frame_builder;
/// Reused visible-row publication from the Session owner to the frame builder.
pub mod frame_snapshot;
/// Converts shaped placements into Slug instances for real terminal rows.
mod glyph_ink;
/// Bounded kitty graphics state and platform decode values (M15).
pub mod images;
/// Shared shortcut grammar, action catalogue and sequence interception.
pub mod keybind;
/// Domain-owned typed bus messages and reserved public protocol names.
pub mod messages;
/// Generic bounded OSC handlers registered on the Session owner (D-44).
pub mod osc;
/// Host services and manual-clock seams for deterministic sessions.
pub mod platform;
/// Shared pointer-selection intent and signed outside-grid coordinates.
pub mod pointer;
/// Owns the VT, typed bus and host services for one terminal instance.
pub mod session;
/// Typed settings, validation reports and layered configuration.
pub mod settings;
/// Built-in and custom terminal palettes with OKLab-derived UI tokens.
pub mod theme;
/// Anchored scrollback position and scrollbar data for desktop and web.
pub mod viewport;
/// Owned host results contain no platform callbacks or Session references.
pub use session::host_events::HostEvent;
/// Host operations cross ownership boundaries as values rather than closures.
pub use session::message::{FontMetrics, FrameResult, SessionMessage};
pub use session::{FeedOutcome, Session, SessionConfig};
/// Scenario fixtures compiled only when a native harness or review build requests them.
#[cfg(any(test, feature = "test-scenes"))]
pub mod test_support;
/// Hook types stay available to hosts through the composition dependency edge.
pub use puzzle_bus as bus;
/// Host input origins cross the bus through the composition crate.
pub use puzzle_bus::Origin;
/// Neutral frame instances exposed to the platform shells through composition.
pub use puzzle_frame as frame;
/// Slug mirror types used by native diagnostic scenes through composition.
pub use puzzle_slug as slug;
/// Font data reaches the platform shell without a new dependency edge.
pub use puzzle_text as text;
/// M5 grid model re-exported to the native preview without a new crate edge.
pub use puzzle_vt as vt;
/// Existing scenario API is available only to explicit harness and browser review builds.
#[cfg(any(test, feature = "test-scenes"))]
pub use test_support::{color_scene, pack_scene, slug_scene, text_scene};

/// Incremental literal-search model and typed requests (§6.10).
pub mod search;

/// Shared selection model and typed mutation requests (§6.9).
pub mod selection;

/// Basic touch motion is deterministic and shared with browser adapters.
pub mod touch;

/// Public Rust overlay providers, geometry and declarative painter types.
pub use puzzle_overlay as overlay;

/// Outcome of publishing sealed source values into a caller-owned snapshot slot.
pub use session::publication::PublishOutcome;

/// Escape-sequence extensions grouped by their defining project (D-58).
mod protocols;

/// MIME clipboard transactions and explicit asynchronous host outcomes.
pub mod clipboard;
/// Native drop gesture values, with Session-owned coordinate and data policy.
pub mod dnd;
/// Shared notification admission, lifetime and host feedback, independent of protocol origin.
pub mod notification;

//! Safe native terminal ownership for host applications (D-43, D-50).
//!
//! Session endpoints exchange messages and sealed publications; they never lend
//! mutable terminal state across threads. The harness consumes these same owners
//! through the checked App/Surface facade. Internal owners are not native-host API.
//!
//! Owner modules cannot be reached by a native host, even when diagnostics is enabled:
//! ```compile_fail
//! use puzzle_embed::session_thread::Handle;
//! ```
//! Rendering internals likewise cannot become a second host API:
//! ```compile_fail
//! use puzzle_embed::render_thread::RenderThread;
//! ```
// Diagnostic items are reachable only with test-support. Their containing
// modules remain private in both builds; App/Surface reexports stay reachable.
#![cfg_attr(
    not(feature = "test-support"),
    allow(unreachable_pub, reason = "diagnostic exports are feature-gated")
)]
/// Main-thread embedding lifetime and deferred host event delivery.
pub(crate) mod app;
pub use app::{
    ApiError, App, AppConfig, AppEvent, SnapshotError, SnapshotSize, Surface, SurfaceOptions,
    SurfacePointer, TransportMode,
};
/// Bounded native bitmap-font decoding, absent from the shared text/wasm crates.
pub(crate) mod bitmap;
/// Minimal safe executor for native GPU initialization and diagnostic recovery.
pub(crate) mod block_on;
/// Single-threaded hosts and harnesses build from the sealed-publication seam.
#[cfg_attr(
    not(feature = "test-support"),
    allow(dead_code, reason = "preserved harness diagnostic helpers")
)]
pub(crate) mod drawing;
/// Approved bundled font loading shared by the harness and owner tests.
pub(crate) mod font_data;
/// Shared GPU test exclusion, excluded from normal host builds.
#[cfg(any(test, feature = "test-support"))]
pub(crate) mod gpu_test;
/// Complete GPU Slug masks use the App device and queue without readback.
pub(crate) mod grid_rasterizer;
/// Native-only asynchronous decoding of bounded direct graphics payloads.
pub(crate) mod image_decode;
/// Neutral host callback facts also used by diagnostic recordings.
pub(crate) mod input;
/// Native key translation preserves physical, layout and committed-text identities.
#[cfg_attr(
    not(feature = "test-support"),
    allow(dead_code, reason = "preserved harness diagnostic helpers")
)]
pub(crate) mod keyboard;
/// Joined native worker lifetime, including startup failure and held EOF.
pub(crate) mod pipeline;
/// Runtime input clocks share cold counters; feed probes remain compile-time opt-in.
pub(crate) mod pipeline_metrics;
/// Lossless diagnostic image output shares one dependency-free implementation.
pub(crate) mod png;
/// Puzzle-owned native process and ordered input writer.
#[cfg_attr(
    not(feature = "test-support"),
    allow(dead_code, reason = "preserved harness diagnostic helpers")
)]
pub(crate) mod pty;
/// Bounded PTY byte ownership and nonblocking control wakeups.
#[cfg_attr(
    not(feature = "test-support"),
    allow(dead_code, reason = "preserved harness diagnostic helpers")
)]
pub(crate) mod pty_pool;
/// Opt-in private recording and exact replay conversion.
#[cfg_attr(
    not(feature = "test-support"),
    allow(dead_code, reason = "preserved harness diagnostic helpers")
)]
pub(crate) mod recording;
/// One process-owned render worker and host-neutral surface mailbox.
pub(crate) mod render_thread;
/// Disabled-by-default resize-stage observation.
pub(crate) mod resize_timing;
/// Fixed-pool session owner, priority messages and immutable frame publications.
#[cfg_attr(
    not(feature = "test-support"),
    allow(dead_code, reason = "preserved harness diagnostic helpers")
)]
pub(crate) mod session_thread;
/// Validated native configuration, profiles and transient zoom snapshots.
#[cfg_attr(
    not(feature = "test-support"),
    allow(dead_code, reason = "preserved harness diagnostic helpers")
)]
#[cfg_attr(
    not(feature = "test-support"),
    allow(unreachable_pub, reason = "JSON replay is feature-gated diagnostics")
)]
pub(crate) mod settings;
/// Native configuration restores platform compositor colour metadata.
pub(crate) mod surface_config;
/// Native discovery and bounded mappings belong to the render-side text owner.
#[cfg_attr(
    not(feature = "test-support"),
    allow(dead_code, reason = "preserved harness diagnostic helpers")
)]
pub(crate) mod system_fonts;
/// Functional tests use a deadlock guard, separate from optimized timing limits.
#[cfg(test)]
mod test_support;
/// Paired CPU clock for explicitly instrumented native workloads.
pub(crate) mod thread_cpu;
/// Native surface execution uses copied host facts rather than toolkit handles.
#[cfg_attr(
    not(feature = "test-support"),
    allow(dead_code, reason = "preserved harness diagnostic helpers")
)]
pub(crate) mod window;

/// Owned host callback facts consumed by Surface::input; they retain layout and physical identity.
pub use input::{LEFT_OPTION, RIGHT_OPTION, RawIme, RawInput, RawKey, RawLogical};
/// Notification callback values and completion tags are part of the Surface host contract.
pub use puzzle_term::notification::{
    Effect as NotificationEffect, Feedback as NotificationFeedback,
};
/// Immutable count/navigation payload delivered by App::tick after Surface search messages.
pub use puzzle_term::search::{SearchState, SearchUi};
/// Parsed, sanitized notification options carried by NotificationEffect::Show.
pub use puzzle_term::vt::notification::{Notification, Occasion as NotificationOccasion};
/// Copied surface geometry and lifecycle observations; no actor endpoint is exposed.
pub use render_thread::{
    host::{HostAction as SurfaceAction, Visibility},
    surface::{Extent, Refresh, SurfaceId, SurfaceInfo},
};
/// Validated native settings used by AppConfig and per-surface updates.
pub use settings::DesktopSettings;
/// Developer-only access for reproducible harness probes; this is not host API.
#[cfg(feature = "test-support")]
pub mod diagnostics;

/// Host drop registration and gesture data; no internal Session API is exposed.
pub use puzzle_term::dnd::{Effect as DropEffect, Input as DropInput, Kind as DropKind};

// Distribution-specific exports are isolated from the supported base API.
include!("optional_items.rs");

/// Optional native event clocks for per-input delivery diagnostics (D-61).
mod input_timing;
pub use input_timing::InputTiming;

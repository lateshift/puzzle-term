//! Public augmented-text geometry and retained providers (design §11).
//!
//! This crate knows VT identities and plain painter data, never a GPU or a
//! platform. Session owns input/clock policy; the render-owned FrameBuilder prepares
//! provider commands in the shared text/Slug stores after sampling VT state.
/// Tracked text, cursor, zone, viewport and pointer attachments.
pub mod anchor;
/// Injected-clock easing without implicit timers.
pub mod animation;
/// Bounded sealed contents window for render-side view callbacks.
pub mod buffer;
/// Built-in model overlays use the same extension provider seam.
pub mod builtins;
/// Declarative annotations rendered by one built-in provider.
pub mod decoration;
/// Anchor-relative painter commands and portable pointer shapes.
pub mod draw;
/// Reversible source-row projection; source text remains in the terminal.
pub mod folding;
/// Backend-neutral cell and glyph placement queries.
pub mod geometry;
/// Session-owned interaction and immutable view-state publication (D-50).
pub mod model;
/// Frozen source models and typed pointer operations, supplied by Session.
pub mod models;
/// Host UI exclusions constrain floating chrome without changing the grid.
pub mod occlusion;
/// Rare extension callbacks, theme tokens and animation requests.
pub mod provider;
/// Dirty interest filtering and retained provider draw lists.
pub mod registry;
/// Retained immutable paths shared with the host's vector preparation cache.
pub mod shapes;
pub use model_registry::ModelRegistry as OverlayRegistry;

/// Free text can be measured without exposing the host's font or atlas owners.
pub mod text;

/// Renderer-owned overlay drawing, independent of interaction and scheduling.
pub mod view;

/// Session-side dirty-state and pointer owners, independent of drawing.
pub mod model_registry;
/// Region tables committed only after a frame is presented.
pub mod regions;
/// Render-side views and immutable values carried by rotating slots.
pub mod view_registry;

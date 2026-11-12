//! Font shaping and backend-neutral glyph placement (design §8).
//!
//! The existing outline probe remains useful for reference scenes. M4 adds
//! the D-28 seam before layout or rendering: callers retain placements,
//! while a selected backend prepares their drawable representation.
/// Cell-sized procedural line, block, Braille and fallback outlines.
pub mod builtin;
/// Parsed face metadata and outline extraction from caller-owned font bytes.
pub mod face;
/// Platform-supplied bytes shared by the M3 probe and later font loading.
mod font_data;
/// Primary faces, fallback order and glyph lookup for a complete cluster.
pub mod font_set;
/// Stable positions and the vector/raster backend seam (design §8.9).
pub mod glyph;
/// D-27 selection of hinted outlines without changing grid positions.
pub mod hinting;
/// UTF-8 clusters placed on integer terminal cells through the glyph seam.
pub mod layout;
/// Font-table metrics converted into integral device-pixel cells.
pub mod metrics;
/// Versioned, validated ready-to-upload Slug font pack data.
pub mod pack;
/// Minimal real-font outline extraction for M2's CPU quality sheet.
pub mod probe;
/// Harfrust shaping with a direct ASCII path and bounded run cache.
pub mod shape;
pub use font_data::FontData;

/// Complete-cluster emoji and platform fallback traits.
pub mod cluster_source;
/// Device-pixel ink geometry shared by vector and bitmap backends.
pub mod fitting;
/// Unicode presentation choice, separate from grid width.
pub mod presentation;

/// COLRv0/v1 painter emitting ordered, palette-aware vector layers.
pub mod color;

/// Bounded shelf pages and generational handles for platform bitmap clusters.
pub mod raster;

/// Embedded sbix/CBDT strike selection and alpha-correct bitmap fitting.
pub mod bitmap;

/// Borrowed native font parsing; platform discovery and mapping stay outside the core.
pub mod cluster_font;

/// Geometry types let platform probes consume owned paths without a second crate edge.
pub use kurbo as geometry;

/// Bounded asynchronous discovery adapter, with provisional tofu until completion.
pub mod deferred;
/// Cold loaded-face preparation uses the host's refresh admission and worker.
pub mod preparation;

/// Opt-in per-layer lookup evidence without a timer or platform dependency.
pub mod cache_metrics;

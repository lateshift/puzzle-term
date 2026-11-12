//! CPU geometry and the Slug pixel-shader oracle (design §9).
//!
//! This leaf crate converts kurbo outlines into quadratic contours, builds
//! ray bands, packs CPU texture mirrors, and evaluates the reference shader
//! without a GPU. M3 will upload these mirrors and compare WGSL with this
//! implementation; no font or platform code belongs here.
/// CPU mirrors, packed addresses and generational shape handles.
pub mod atlas;
/// Equal-thickness curve lists for each ray direction.
pub mod bands;
/// Guarded device-space partitions for large or dense overlay outlines.
pub mod chunks;
/// Authoritative CPU packing and unpacking of the instance word fields.
pub mod encoding;
/// Pixel-shader coverage ported from Eric Lengyel's reference HLSL.
pub mod reference;
/// Closed, size-independent quadratic outlines in em coordinates.
pub mod shape;
/// Synthetic kurbo paths for tests, debugging and benchmarks.
pub mod synthetic;

pub use atlas::{ShapeHandle, ShapeKey, SlugAtlas};
pub use shape::{DEFAULT_TOLERANCE, QuadCurve, QuadShape, ShapeError};

/// COLR/overlay color brushes, their table and CPU color oracle.
pub mod paint;

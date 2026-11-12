//! GPU-independent instance data shared by the core and renderer (§9.5, §10.2).
//!
//! The Slug instance is plain bytes: its em-space bounds and transform do
//! not depend on a GPU device. M6 adds the full frame contract around it.
/// Owned per-frame arrays and ordered GPU draw batches.
pub mod contract;
/// A glyph or image copied from a mask or color atlas.
pub mod raster;
/// A background, selection, cursor or straight decoration quad.
pub mod rect;
/// One outline quad and its packed atlas addresses.
pub mod slug;

pub use contract::{Batch, Globals, PipelineKind, RectU32, RenderFrame, Upload, ZSlot};
pub use raster::RasterInstance;
pub use rect::RectInstance;
pub use slug::SlugInstance;

/// Sealed RGBA sources and image raster primitives (M15).
pub mod image;
pub use image::{FrameImages, ImageSource, RasterRef};

/// Device-neutral tiling arithmetic shared by admission and rendering.
mod image_layout;
pub use image_layout::{image_resident_bytes, image_tile_border, image_tile_step};

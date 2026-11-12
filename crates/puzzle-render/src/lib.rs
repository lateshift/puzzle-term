//! Minimal WebGPU rendering shared by the browser and native harness (design §10.1).
//!
//! Platform shells own instances and surfaces; this crate owns the device and clear pass.
/// Ordered single-pass execution of GPU-neutral render frames.
mod frame_renderer;
/// R8 mask and RGBA color atlas quads behind the glyph backend seam.
mod raster_pipeline;
/// Offscreen transfer from GPU memory into a tightly packed CPU image.
mod readback;
/// Instanced rectangles for backgrounds, decorations and cursors.
mod rect_pipeline;
/// Device initialization, surface configuration, and the M0 clear pass.
mod renderer;
/// Native D-8 queue-completion comparison of float texture formats.
#[cfg(feature = "oracle-debug")]
mod slug_bench;
/// Native-only fragment input capture for the mathematical oracle layer.
#[cfg(feature = "oracle-debug")]
mod slug_debug;
/// Native offscreen Slug draw path used by GPU oracle comparisons.
mod slug_offscreen;
/// Instanced Slug draw state and the faithful WGSL port.
mod slug_pipeline;
/// Presentable Slug pass for the browser's M3 review scene.
mod slug_surface;
/// GPU textures mirroring the Slug curve and band planes.
mod slug_texture;
pub use frame_renderer::{FramePipelines, FrameRenderer};
pub use raster_pipeline::RasterPipeline;
pub use readback::RgbaImage;
pub use rect_pipeline::RectPipeline;
pub use renderer::{CLEAR, RenderError, Renderer};
#[cfg(feature = "oracle-debug")]
pub use slug_debug::SlugDebugPixel;
pub use slug_pipeline::SlugPipeline;
pub use slug_texture::SlugTextures;

/// Persistent Slug resources bake cold device-size masks in bounded batches.
mod mask_baker;
pub use mask_baker::MaskBaker;

/// Product uncaptured-error recovery with a strict acceptance hook.
mod errors;
pub use errors::{ErrorMonitor, FrameContext};

/// Surface-owned, rebuildable image residency, separate from glyph text contexts.
mod images;
pub use images::ImageCache;

/// Real swapchain evidence, separate from the offscreen golden oracle.
mod surface_readback;

/// One encoded-colour decision shared by configure and pipeline setup.
mod surface_config;

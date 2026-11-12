//! Every native configure restores compositor metadata that wgpu resets.
//!
//! This cold seam covers initial creation, resize, validation recovery and device
//! recreation for both winit views and borrowed host layers. Frame execution and
//! drawable bytes stay unchanged; OS metadata belongs outside the shared renderer.
use puzzle_render::Renderer;

/// Configures backing pixels, then explicitly tags the native layer as sRGB.
/// Failure propagates before drawing; no surface silently retains device-space ink.
pub(crate) fn configure(
    renderer: &Renderer,
    surface: &wgpu::Surface<'_>,
    width: u32,
    height: u32,
) -> Result<(), String> {
    // wgpu must go first: Metal configure replaces colorspace with nil for Srgb.
    renderer
        .configure(surface, width, height)
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    {
        // Use the very layer held by the surface, not its host view or an ancestor.
        puzzle_sys::layer::colour::set_surface_srgb(surface)?;
        tracing::debug!(
            event = "desktop.layer_colorspace",
            profile = "sRGB",
            width,
            height
        );
    }
    Ok(())
}

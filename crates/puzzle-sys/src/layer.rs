//! D-54: a resized native layer preserves old drawable pixels without scaling.
//!
//! The wgpu Surface keeps the native view and Metal layer alive. We borrow its
//! HAL guard and layer lock only while setting one Core Animation property.
//! No object is destroyed, released or replaced, and no drawable is acquired.
//! Gravity names follow the contents' vertical axis, not a fixed visual corner.
//! contentsAreFlipped includes inherited host orientation; applying another XOR
//! with geometryFlipped would count that orientation twice. Neither flag changes.
/// Named sRGB metadata is restored after wgpu surface configuration.
pub mod colour;

use objc2_quartz_core::{CALayer, kCAGravityBottomLeft, kCAGravityTopLeft};

/// Core Animation's named alignment, before interpreting the contents' orientation.
#[derive(Debug, PartialEq, Eq)]
pub enum GravityCorner {
    /// Visual top left when the layer's contents are not implicitly flipped.
    TopLeft,
    /// Visual top left when the layer's contents are implicitly flipped.
    BottomLeft,
}

/// Readback of the retained layer, for startup/recreation evidence.
#[derive(Debug)]
pub struct GravityReading {
    /// Confirms that the retained gravity selects the effective visual top left.
    pub top_left: bool,
    /// The named constant selected after resolving the contents' orientation.
    pub corner: GravityCorner,
    /// Layer coordinate orientation, preserved rather than changed by this API.
    pub geometry_flipped: bool,
    /// Contents orientation, likewise left under the surface owner's control.
    pub contents_flipped: bool,
    /// Immediate host coordinates, for diagnosing backing layers versus sublayers.
    pub host_geometry_flipped: Option<bool>,
    /// Host contents orientation; already accounted for by contentsAreFlipped.
    pub host_contents_flipped: Option<bool>,
}

/// Sets and reads the Metal surface's contents gravity without exposing a raw layer.
/// Returns an error for another backend or a rejected property readback.
#[allow(
    unsafe_code,
    reason = "R8/D-54: scoped wgpu HAL access borrows a live layer"
)]
pub fn set_surface_contents_top_left(
    surface: &wgpu::Surface<'_>,
) -> Result<GravityReading, String> {
    // SAFETY: the borrowed Surface outlives this guard. We never destroy or
    // replace its resources, acquire a drawable or modify HAL ownership/state.
    let hal =
        unsafe { surface.as_hal::<wgpu::hal::api::Metal>() }.ok_or("surface has no Metal layer")?;
    // The HAL mutex serializes property access with its configure/acquire path.
    let layer = hal.render_layer().lock();
    set_layer(&layer)
}

/// Applies the visual corner without changing transforms, bounds or flip flags.
#[allow(
    unsafe_code,
    reason = "R8/D-54: immutable OS gravity constant has process lifetime"
)]
pub(crate) fn set_layer(layer: &CALayer) -> Result<GravityReading, String> {
    // Apple's contentsGravity documentation explicitly reverses top/bottom when
    // contentsAreFlipped is true. That getter includes the host's inherited flip:
    // https://developer.apple.com/documentation/quartzcore/calayer/contentsgravity
    let contents_flipped = layer.contentsAreFlipped();
    let host = layer.superlayer();
    // SAFETY: this framework-provided immutable NSString is present on all
    // supported macOS versions and retained for the lifetime of the process.
    let gravity = unsafe {
        if contents_flipped {
            kCAGravityBottomLeft
        } else {
            kCAGravityTopLeft
        }
    };
    // The generated safe binding copies the string and does not transfer ownership.
    layer.setContentsGravity(gravity);
    let reading = GravityReading {
        top_left: &*layer.contentsGravity() == gravity,
        corner: if contents_flipped {
            GravityCorner::BottomLeft
        } else {
            GravityCorner::TopLeft
        },
        geometry_flipped: layer.isGeometryFlipped(),
        contents_flipped,
        host_geometry_flipped: host.as_ref().map(|host| host.isGeometryFlipped()),
        host_contents_flipped: host.as_ref().map(|host| host.contentsAreFlipped()),
    };
    // A silently rejected property must not be reported as applied.
    if !reading.top_left {
        return Err("Metal layer rejected visual top-left contents gravity".into());
    }
    Ok(reading)
}

#[cfg(test)]
/// Effective alignment covers inherited and cancelling coordinate flips.
mod tests;

/// Copies native layer and drawable extents for opt-in resize diagnostics.
/// Values are logical points, backing scale and drawable device pixels respectively.
#[allow(
    unsafe_code,
    reason = "R8/D-54: read-only geometry through borrowed Metal HAL"
)]
pub fn surface_geometry(surface: &wgpu::Surface<'_>) -> Option<([f64; 2], f64, [f64; 2])> {
    // SAFETY: Surface remains borrowed for the guard's lifetime. No HAL resources
    // are destroyed/replaced and no native ownership or queue state is changed.
    let hal = unsafe { surface.as_hal::<wgpu::hal::api::Metal>() }?;
    let layer = hal.render_layer().lock();
    // Property getters copy scalar geometry; neither retains a drawable nor waits for one.
    let bounds = layer.bounds();
    let drawable = layer.drawableSize();
    Some((
        [bounds.size.width, bounds.size.height],
        layer.contentsScale(),
        [drawable.width, drawable.height],
    ))
}

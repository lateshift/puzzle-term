//! Explicit compositor matching for D-27's encoded sRGB drawable bytes.
//!
//! wgpu's Metal configure clears colorspace to nil for SurfaceColorSpace::Srgb.
//! Nil leaves saturated colours in device space on P3 displays. Reapply the named
//! profile after every configure; changing the attachment or shader is unrelated.
use objc2_core_graphics::{CGColorSpace, kCGColorSpaceSRGB};
use objc2_quartz_core::CAMetalLayer;

/// Tags the actual retained Metal layer, including host-owned CAMetalLayers.
/// Call after wgpu configure, which otherwise replaces this property with nil.
#[allow(unsafe_code, reason = "R8/D-54: borrowed Metal HAL layer metadata")]
pub fn set_surface_srgb(surface: &wgpu::Surface<'_>) -> Result<(), String> {
    // SAFETY: Surface outlives this guard. No resource is destroyed, replaced,
    // acquired or submitted; the HAL lock serializes the native property access.
    let hal =
        unsafe { surface.as_hal::<wgpu::hal::api::Metal>() }.ok_or("surface has no Metal layer")?;
    set_layer_srgb(&hal.render_layer().lock())
}

/// Reads native metadata independently of wgpu's requested configuration.
/// Nil is deliberately false: it cannot prove sRGB compositor matching.
#[allow(unsafe_code, reason = "R8/D-54: read-only borrowed Metal layer")]
pub fn surface_is_srgb(surface: &wgpu::Surface<'_>) -> Result<bool, String> {
    // SAFETY: this borrowed guard cannot outlive Surface. Only a property is read;
    // drawable ownership, configuration and queue state are untouched.
    let hal =
        unsafe { surface.as_hal::<wgpu::hal::api::Metal>() }.ok_or("surface has no Metal layer")?;
    Ok(layer_is_srgb(&hal.render_layer().lock()))
}

/// Generated CoreGraphics bindings own creation; no handwritten foreign call.
#[allow(unsafe_code, reason = "R8/D-54: process-lifetime CoreGraphics name")]
fn set_layer_srgb(layer: &CAMetalLayer) -> Result<(), String> {
    // SAFETY: Apple's immutable sRGB CFString is available on every supported
    // macOS version and remains valid for the entire process lifetime.
    let name = unsafe { kCGColorSpaceSRGB };
    let colour = CGColorSpace::with_name(Some(name)).ok_or("cannot create sRGB colour space")?;
    // CAMetalLayer retains the profile through its generated safe setter. The
    // local creation reference can drop without invalidating the layer's copy.
    layer.setColorspace(Some(&colour));
    if !layer_is_srgb(layer) {
        return Err("Metal layer rejected explicit sRGB colour space".into());
    }
    Ok(())
}

/// Compare the actual named profile, never the absence of wide-gamut metadata.
#[allow(unsafe_code, reason = "R8/D-54: immutable framework colour-space name")]
fn layer_is_srgb(layer: &CAMetalLayer) -> bool {
    let colour = layer.colorspace();
    let name = CGColorSpace::name(colour.as_deref());
    // SAFETY: the framework's constant name is immutable, live for the process,
    // and borrowed only for this comparison against the retained readback.
    let srgb = unsafe { kCGColorSpaceSRGB };
    name.as_deref() == Some(srgb)
}

#[cfg(test)]
/// Configuration resets and inherited P3 profiles must both be corrected.
mod tests;

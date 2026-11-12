//! A nil profile is not sRGB; configure/reconfigure must restore a named profile.
use super::*;

#[test]
fn configure_and_resize_restore_explicit_srgb() -> Result<(), String> {
    let layer = CAMetalLayer::new();
    assert!(!layer_is_srgb(&layer), "nil must fail the native contract");
    // This is wgpu's configure behavior, repeated for creation and two resizes.
    for _ in 0..3 {
        layer.setColorspace(None);
        assert!(!layer_is_srgb(&layer));
        set_layer_srgb(&layer)?;
        assert!(layer_is_srgb(&layer));
    }
    Ok(())
}

#[test]
#[allow(unsafe_code, reason = "R8/D-54: read immutable test profile name")]
fn borrowed_wide_gamut_layer_becomes_srgb() -> Result<(), String> {
    // SAFETY: the OS-owned Display P3 name is immutable and process-lived;
    // the generated binding performs and owns colour-space creation safely.
    let name = unsafe { objc2_core_graphics::kCGColorSpaceDisplayP3 };
    let p3 = CGColorSpace::with_name(Some(name)).ok_or("cannot create P3 test profile")?;
    let layer = CAMetalLayer::new();
    layer.setColorspace(Some(&p3));
    assert!(!layer_is_srgb(&layer));
    set_layer_srgb(&layer)?;
    assert!(layer_is_srgb(&layer));
    Ok(())
}

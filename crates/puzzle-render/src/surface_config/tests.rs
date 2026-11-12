//! D-27 attachment selection reproduces Metal's sRGB-first capability order.
use super::*;
use wgpu::TextureFormat::*;

#[test]
fn encoded_surface_format_never_double_encodes() {
    // Metal advertises sRGB first. Pipeline creation must choose its Unorm sibling.
    assert!(matches!(
        encoded_format(Bgra8UnormSrgb, &[Bgra8UnormSrgb, Bgra8Unorm]),
        Ok(Bgra8Unorm)
    ));
    assert!(matches!(
        encoded_format(Rgba8UnormSrgb, &[Rgba8UnormSrgb, Rgba8Unorm]),
        Ok(Rgba8Unorm)
    ));
    // Already encoded attachments are stable, including WebGPU's preferred format.
    assert!(matches!(
        encoded_format(Bgra8Unorm, &[Bgra8Unorm]),
        Ok(Bgra8Unorm)
    ));
    assert!(matches!(
        encoded_format(Bgra8UnormSrgb, &[Bgra8UnormSrgb, Rgba8Unorm]),
        Ok(Rgba8Unorm)
    ));
    assert!(encoded_format(Bgra8UnormSrgb, &[Bgra8UnormSrgb]).is_err());
    assert!(encoded_format(Bgra8Unorm, &[]).is_err());
}

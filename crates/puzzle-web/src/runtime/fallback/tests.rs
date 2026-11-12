//! White antialiased text is coverage; visible authored RGB is color (§8.9).
use super::*;
use puzzle_term::text::glyph::BitmapKind;
use wasm_bindgen_test::wasm_bindgen_test;
/// Paired backing-policy measurements retain the same reusable geometry.
mod readback;

/// Fractional white edges and invisible RGB must not accidentally select RGBA.
#[wasm_bindgen_test]
fn pixel_kind_and_unsupported_clusters() {
    let data = [250, 0, 7, 0, 255, 255, 255, 127, 255, 255, 255, 255];
    let mask = pixels::crop(&data, 3, 1, 2).unwrap();
    assert_eq!(mask.kind, BitmapKind::Mask);
    assert_eq!(mask.pixels, [127, 255]);
    assert_eq!(mask.offset_px, [1, -2]);
    // Black emoji outlines remain authored color even though their channels agree.
    let color = pixels::crop(&[0, 0, 0, 127], 1, 1, 0).unwrap();
    assert_eq!(color.kind, BitmapKind::Color);
    assert_eq!(color.pixels, [0, 0, 0, 127]);
    let mut source = BrowserFallback::new(&[]);
    // Multiple unsupported scalars cannot claim coverage by differing from one box.
    let request = ClusterRequest {
        cluster: "\u{10ffff}\u{200d}\u{10ffff}",
        style: FontStyle::Regular,
        presentation: Presentation::Text,
    };
    assert!(!PlatformFallback::can_draw(&mut source, request));
}

//! Reuse must clear old ink and transforms without replacing the JS context.
use super::*;
use wasm_bindgen_test::wasm_bindgen_test;

/// A resized and then reused stage matches a fresh stage pixel for pixel.
#[wasm_bindgen_test]
fn shared_readback_clears_old_ink_and_transform() {
    let mut surface = Surface::default();
    let (first, created) = surface.get(32, 36).unwrap();
    assert!(created);
    first.set_transform(2.0, 0.0, 0.0, 2.0, 7.0, 5.0).unwrap();
    first.set_fill_style_str("#ff0000");
    first.fill_rect(0.0, 0.0, 32.0, 36.0);
    for (w, h) in [(48, 42), (48, 42), (32, 36)] {
        let (reused, created) = surface.get(w, h).unwrap();
        assert!(!created);
        assert!(js_sys::Object::is(first.as_ref(), reused.as_ref()));
        let fresh = crate::runtime::fallback::context(w, h).unwrap();
        for context in [&reused, &fresh] {
            context.set_font("24px system-ui");
            context.fill_text("漢", 0.0, 28.0).unwrap();
        }
        let read = |c: &OffscreenCanvasRenderingContext2d| {
            c.get_image_data(0.0, 0.0, f64::from(w), f64::from(h))
                .unwrap()
                .data()
                .0
        };
        assert_eq!(read(&reused), read(&fresh));
        // Leave foreign state behind before the next same-size reuse.
        reused.set_transform(1.0, 0.0, 0.0, 1.0, 11.0, 0.0).unwrap();
        reused.set_fill_style_str("#0000ff");
        reused.fill_rect(0.0, 0.0, 48.0, 42.0);
    }
}

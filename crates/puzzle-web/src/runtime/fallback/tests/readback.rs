//! Paired reusable Canvas surfaces isolate the readback hint at real device sizes.
use super::super::*;
use wasm_bindgen_test::{console_log, wasm_bindgen_test};

/// Every sample contains one draw/readback; counts mirror D-33's 32-sample history.
#[wasm_bindgen_test]
fn readback_hint_cost_on_reused_canvas() {
    let dpr = web_sys::window().unwrap().device_pixel_ratio();
    let width = (32.0 * dpr) as u32;
    let height = (36.0 * dpr) as u32;
    // Both contexts exist before sampling. Their first-use costs remain in the
    // raw vectors; no warmup samples are removed from the percentile evidence.
    let surfaces =
        [false, true].map(|hint| context_with_readback_hint(width, height, hint).unwrap());
    let mut draw = [Vec::new(), Vec::new()];
    let mut read = [Vec::new(), Vec::new()];
    for i in 0..32 {
        // Alternate order to avoid always assigning a colder system-font call
        // to one backing policy. Both see the same CJK/emoji/Latin requests.
        for policy in [i % 2, 1 - i % 2] {
            let c = &surfaces[policy];
            c.clear_rect(0.0, 0.0, f64::from(width), f64::from(height));
            c.set_font(&format!("{}px system-ui", 24.0 * dpr));
            let text = ["漢", "한", "か", "🦀", "語", "글", "な", "✌️"][i % 8];
            let start = web_time::Instant::now();
            c.fill_text(text, 0.0, 28.0 * dpr).unwrap();
            draw[policy].push(start.elapsed().as_secs_f64() * 1000.0);
            let start = web_time::Instant::now();
            let pixels = c
                .get_image_data(0.0, 0.0, f64::from(width), f64::from(height))
                .unwrap()
                .data()
                .0;
            read[policy].push(start.elapsed().as_secs_f64() * 1000.0);
            assert!(
                pixels.chunks_exact(4).any(|p| p[3] > 0),
                "readback lost covered ink"
            );
        }
    }
    console_log!(
        "D39_READBACK {{\"dpr\":{dpr},\"draw_ms\":{draw:?},\"read_ms\":{read:?},\"hint\":[false,true]}}"
    );
}

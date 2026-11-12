//! Release Chrome budget witnesses; dev-page and acceptance call the same probes.
#![allow(
    clippy::unwrap_used,
    reason = "benchmark setup failures must fail the witness"
)]
use super::*;
use wasm_bindgen_test::{console_log, wasm_bindgen_test};

/// Plain ASCII includes CR/LF scrolling rather than decoding into a sink.
#[wasm_bindgen_test]
fn ascii_parse_budget() {
    let rate = parse("plain-ascii").unwrap();
    console_log!("M7_METRIC ascii_mbps={rate:.3}");
    assert!(rate.is_finite() && rate >= 100.0, "ASCII {rate} MB/s < 100");
}

/// SGR-heavy includes rendition and grid mutations at each source record.
#[wasm_bindgen_test]
fn sgr_parse_budget() {
    let rate = parse("sgr-heavy").unwrap();
    console_log!("M7_METRIC sgr_mbps={rate:.3}");
    assert!(rate.is_finite() && rate >= 40.0, "SGR {rate} MB/s < 40");
}

/// An unchanged cached row cannot stand in for either specified redraw budget.
#[wasm_bindgen_test]
fn frame_cpu_budgets() {
    let metrics = frame::measure().unwrap();
    console_log!(
        "M7_METRIC full_frame_ms={:.3} row_frame_ms={:.3}",
        metrics[0],
        metrics[1]
    );
    assert!(
        metrics[0].is_finite() && metrics[0] <= 4.0,
        "full {} ms > 4",
        metrics[0]
    );
    assert!(
        metrics[1].is_finite() && metrics[1] <= 0.5,
        "row {} ms > 0.5",
        metrics[1]
    );
}

/// Scrolling includes the extra clipped row and one newly exposed full-width row.
#[wasm_bindgen_test]
fn scrolling_frame_cpu_budget() {
    let millis = frame::scroll().unwrap();
    console_log!("M8_METRIC scroll_frame_ms={millis:.3}");
    assert!(
        millis.is_finite() && millis <= 0.5,
        "scroll {millis} ms > 0.5"
    );
}

/// Search yields between actual refreshes and reports all 100,001 examined rows.
#[wasm_bindgen_test(async)]
async fn search_slice_and_total_measurement() {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    let mut probe = search::SearchBenchmark::new();
    let mut slices = Vec::new();
    let mut rows = 0;
    let start = Instant::now();
    while !probe.complete() {
        JsFuture::from(js_sys::Promise::new(&mut |resolve, _| {
            web_sys::window()
                .unwrap()
                .request_animation_frame(resolve.unchecked_ref())
                .unwrap();
        }))
        .await
        .unwrap();
        let sample = probe.step();
        assert!(sample[0].is_finite() && sample[0] >= 0.0);
        assert!(sample[1] <= 2_000.0);
        rows += sample[1] as usize;
        slices.push(sample[0]);
    }
    let wall = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(rows, 100_001);
    assert_eq!(slices.len(), 51);
    assert_eq!(probe.count(), 1_000);
    let total: f64 = slices.iter().sum();
    let maximum = slices.iter().copied().fold(0.0, f64::max);
    slices.sort_by(f64::total_cmp);
    // No budget is invented for search; the mechanical allowance and timing are separate.
    console_log!(
        "M8_METRIC search_slice_median_ms={:.3} search_slice_max_ms={maximum:.3} search_cpu_total_ms={total:.3} search_wall_ms={wall:.3} slices=51 rows={rows} count=1000",
        slices[25]
    );
}

//! Browser parser-and-grid measurements on the native probe's same corpora.
//!
//! Five warmed 8 MiB samples include decoding, rendition, cell writes and
//! scrolling. Construction and corpus repetition stay outside the timer.
//! The exported functions exist only in an explicitly enabled dev build;
//! normal releases include neither benchmark code nor embedded fixtures.
use puzzle_term::vt::Terminal;
use std::hint::black_box;
use wasm_bindgen::prelude::*;
use web_time::Instant;

/// Complete source records are repeated beyond this size, never truncated.
const SAMPLE_BYTES: usize = 8 * 1024 * 1024;

/// Selects a known deterministic corpus instead of accepting filesystem paths.
fn source(name: &str) -> Result<&'static [u8], JsValue> {
    match name {
        "plain-ascii" => Ok(include_bytes!(
            "../../../assets/fixtures/corpus/plain-ascii.vt"
        )),
        "sgr-heavy" => Ok(include_bytes!(
            "../../../assets/fixtures/corpus/sgr-heavy.vt"
        )),
        _ => Err(js_sys::Error::new("unknown parse benchmark corpus").into()),
    }
}

/// Returns the five-sample median in decimal MB/s, including actual grid work.
/// This blocks the dev page briefly; it is a measurement, never a runtime path.
#[cfg_attr(feature = "measurements", wasm_bindgen(js_name = benchmarkParse))]
pub fn parse(name: &str) -> Result<f64, JsValue> {
    let source = source(name)?;
    let mut bytes = Vec::with_capacity(SAMPLE_BYTES + source.len());
    // Preserve complete escape sequences at each repeated corpus boundary.
    while bytes.len() < SAMPLE_BYTES {
        bytes.extend_from_slice(source);
    }
    // Match the native parse probe's 80×24 geometry and 100 history rows.
    let mut terminal = Terminal::new(80, 24, 100, 80 * 16 * 100);
    terminal.advance(source);
    let mut rates = [0.0; 5];
    for rate in &mut rates {
        // Do not time fixture construction, formatting, or JS object conversion.
        let start = Instant::now();
        terminal.advance(black_box(&bytes));
        *rate = bytes.len() as f64 / start.elapsed().as_secs_f64() / 1_000_000.0;
    }
    rates.sort_by(f64::total_cmp);
    // Reading the state also makes the grid side of the probe observable.
    black_box(terminal.cursor_line());
    Ok(rates[2])
}

/// CPU frame assembly uses the same warmed fixture as the native frame probe.
pub mod frame;
/// Identical native/browser incremental-query workload with separately timed slices.
pub mod search;

#[cfg(test)]
/// Every reported floor is backed by an executed optimized browser witness.
mod tests;

//! Native search-turn CPU timings for the shared 100,000-line fixture (§6.10).
//! Every slice is retained; formatting and fixture construction are outside timers.
//! Native CPU total excludes waits; the Chrome page separately records wall time.
//! The final live blank row is counted too, preventing a shortened history trial.
use puzzle_term::test_support::search_probe::SearchProbe;
use std::io::Write;

/// Reports raw pairs so acceptance can verify the allowance and the full workload.
fn main() -> Result<(), String> {
    let mut probe = SearchProbe::new();
    let mut text = String::new();
    while !probe.complete() {
        let (millis, rows) = probe.step();
        // Writing only after completion prevents terminal I/O from perturbing slices.
        text.push_str(&format!("slice_ms={millis:.6} rows={rows}\n"));
    }
    text.push_str(&format!("count={}\n", probe.count()));
    std::io::stdout()
        .write_all(text.as_bytes())
        .map_err(|e| e.to_string())
}

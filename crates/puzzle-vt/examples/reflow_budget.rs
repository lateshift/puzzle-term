//! Release-mode 10,000×200 reflow budget probe (design §15.2).
//!
//! Setup and byte parsing happen outside the timer. The measured operation
//! is only `Terminal::resize`, including logical-line rewrap, tracked state
//! and history eviction. Five fresh trials give a median that is less
//! sensitive to unrelated scheduler pauses than one timing sample.
//! Every source line fills the old 200-column width before CR/LF, giving
//! the resize path 10,000 hard logical lines rather than one giant wrapped
//! line. The 100-column destination forces each logical line to split.
//! The artifact is removed by xtask before the probe runs, so a stale
//! result cannot make an incomplete benchmark satisfy acceptance (R14).

use std::{fs, time::Instant};

use puzzle_vt::Terminal;

/// Builds 10,000 hard lines at 200 columns without timing construction.
fn fixture() -> Terminal {
    // The generous byte cap retains all source lines before the timer.
    let mut term = Terminal::new(200, 50, 10_000, 80_000_000);
    let line = [b'X'; 200];
    for _ in 0..10_000 {
        // CR cancels delayed wrap before LF commits a hard line ending.
        term.advance(&line);
        term.advance(b"\r\n");
    }
    term
}

/// Stores the median milliseconds for xtask's fresh acceptance check.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut samples = [0.0_f64; 5];
    for sample in &mut samples {
        // Rebuild outside the measured span to time only the resize path.
        let mut term = fixture();
        let started = Instant::now();
        term.resize(100, 50);
        *sample = started.elapsed().as_secs_f64() * 1000.0;
        // The measured resize must have actually changed the ring width.
        if term.size() != (100, 50) {
            return Err("reflow did not resize the visible grid".into());
        }
    }
    samples.sort_by(f64::total_cmp);
    // A single decimal value is the machine-readable acceptance artifact.
    fs::create_dir_all("target/corpus")?;
    fs::write("target/corpus/reflow-ms.txt", format!("{:.6}", samples[2]))?;
    Ok(())
}

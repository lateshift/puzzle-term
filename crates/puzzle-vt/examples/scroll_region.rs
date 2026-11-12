//! Native comparison of a top-origin DEC region scroll and a full-screen LF.
//!
//! Both paths start with 10,000 retained history rows at the same 80×48
//! grid size. The benchmark times terminal control processing rather than
//! fixture construction, exposing accidental O(scrollback) work in SU.
//! The final screen row is excluded from the region because tmux reserves
//! that row for its status line. If SU dirties it, retained frame caches
//! are invalidated even when the row's cells did not move.
//! The baseline LF scrolls the entire viewport while retaining history;
//! its cost is the common terminal operation that SU must approach.
use puzzle_vt::Terminal;
use std::{hint::black_box, time::Instant};

/// Repetitions amortize the clock and loop overhead of one scroll action.
const SCROLLS: usize = 10_000;

/// Fills the history ring before the timed control sequence begins.
fn fixture(region: bool) -> Terminal {
    // The byte cap exceeds the fixed-width 10,000-row history so line
    // count, rather than memory pressure, decides when recycling starts.
    let mut term = Terminal::new(80, 48, 10_000, 32 * 1024 * 1024);
    let line = [b'X'; 80];
    for _ in 0..10_048 {
        // CR resolves pending wrap before LF commits another history row.
        term.advance(&line);
        term.advance(b"\r\n");
    }
    // Both measured paths must begin on a warm, completely full ring.
    assert_eq!(term.screen().ring.scrollback_len(), 10_000);
    if region {
        // The last screen row is tmux's status line, outside DECSTBM 1;47.
        term.advance(b"\x1b[1;47r");
    }
    term
}

/// Reports one control sequence's median cost with fixture work excluded.
fn median_ns(region: bool) -> f64 {
    let mut samples = [0.0_f64; 5];
    for sample in &mut samples {
        // Each trial gets the same initial history, so earlier evictions
        // cannot change the work of a later trial.
        let mut term = fixture(region);
        // CSI S scrolls DECSTBM; CR LF triggers the full-screen line-feed
        // path at the bottom cursor established by the fixture.
        let input: &[u8] = if region { b"\x1b[S" } else { b"\r\n" };
        let before = term.screen().ring.scroll_total();
        let started = Instant::now();
        for _ in 0..SCROLLS {
            term.advance(black_box(input));
        }
        // The control must actually advance a row; otherwise a tiny cost
        // could silently measure a no-op at the wrong cursor position.
        if !region {
            assert_eq!(term.screen().ring.scroll_total() - before, SCROLLS as u64);
        }
        // A visible line identity changes on each scroll. Consume it so
        // the compiler cannot treat the final terminal state as dead.
        black_box(term.screen().ring.newest_id());
        *sample = started.elapsed().as_nanos() as f64 / SCROLLS as f64;
    }
    // Median rejects a single scheduling interruption without hiding
    // persistent work proportional to the 10,000-row history.
    samples.sort_by(f64::total_cmp);
    samples[2]
}

/// Writes a machine-readable result that xtask can use for the budget.
fn main() -> std::io::Result<()> {
    let lf = median_ns(false);
    let su = median_ns(true);
    std::fs::create_dir_all("target/corpus")?;
    std::fs::write(
        "target/corpus/scroll-region-ns.txt",
        format!(
            "full_lf_ns={lf:.3}\nregion_su_ns={su:.3}\nratio={:.3}\n",
            su / lf
        ),
    )
}

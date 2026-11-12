//! Divan timing for 10,000 lines × 200 columns of primary reflow (§15.2).
//!
//! `with_inputs` constructs a fresh, filled terminal outside each timed
//! iteration. The benchmark measures width-change reflow and its history
//! accounting, while fixture byte parsing is excluded. This is the same
//! shape as the release example used by `cargo xtask accept M5`.
//! The new width halves the row capacity, forcing each hard logical line
//! into two physical rows and exercising the wide history-retention path.

use puzzle_vt::Terminal;

/// Registers the reflow fixture with divan's native benchmark runner.
fn main() {
    divan::main();
}

/// Builds complete hard lines so each must be rewrapped independently.
fn fixture() -> Terminal {
    let mut term = Terminal::new(200, 50, 10_000, 80_000_000);
    let line = [b'X'; 200];
    for _ in 0..10_000 {
        // CR cancels delayed wrap and LF commits the hard row boundary.
        term.advance(&line);
        term.advance(b"\r\n");
    }
    term
}

/// Times primary reflow without charging input construction to the budget.
#[divan::bench(sample_count = 10, sample_size = 1)]
fn ten_thousand_by_two_hundred(bencher: divan::Bencher) {
    bencher.with_inputs(fixture).bench_local_values(|mut term| {
        term.resize(100, 50);
        // A consumed result keeps the optimizer from eliding the resize.
        divan::black_box(term.size());
    });
}

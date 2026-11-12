//! Native D-29 snapshot-copy budget probe at 240×70 cells.
//!
//! VT writes and fixture construction are outside each timed interval.
//! Full trials invalidate the style revision; row trials rewrite one
//! physical cell. Both use a retained snapshot whose buffers are warm.
//! The reported median avoids a stray scheduler interruption deciding
//! whether a microsecond-scale copy meets its native frame budget.
//! Keeping the terminal at the product's largest common benchmark size
//! catches a copy loop whose cost scales with all visible cells. Timing
//! a one-row edit separately catches a snapshot that copies all rows
//! even when its metadata reports only one damaged row. The probe omits
//! GPU and Harfrust work because those happen after the desktop lock.
use puzzle_term::frame_snapshot::FrameSnapshot;
use puzzle_vt::terminal::Terminal;
use std::{hint::black_box, io::Write, time::Instant};

/// Fills the same 240×70 viewport used by the frame assembly probe.
fn fixture() -> Terminal {
    let mut term = Terminal::new(240, 70, 0, 0);
    let mut bytes = Vec::with_capacity(70 * 242);
    for row in 0..70 {
        // Distinct rows prevent a malformed copy from appearing valid
        // merely because every source row has identical ASCII content.
        bytes.extend(std::iter::repeat_n(b'A' + row as u8 % 26, 240));
        if row < 69 {
            bytes.extend_from_slice(b"\r\n");
        }
    }
    term.advance(&bytes);
    term
}

/// Measures the snapshot itself, excluding later shaping and GPU work.
fn run() -> Result<(), String> {
    let mut term = fixture();
    let mut snapshot = FrameSnapshot::default();
    snapshot.capture(&term, 0)?;
    let mut full = Vec::with_capacity(101);
    // A fresh epoch is passed on every iteration so all 70 row buffers
    // are copied, without rebuilding the VT fixture inside the timer.
    for trial in 0..101 {
        // An epoch change requires every tagged color and glyph to be
        // captured, while the old row allocations remain available.
        let start = Instant::now();
        snapshot.capture(&term, trial + 1)?;
        full.push(start.elapsed().as_secs_f64() * 1_000_000.0);
        black_box(snapshot.copied_count());
    }
    let mut row = Vec::with_capacity(301);
    // After the final full trial, the epoch stays fixed; only the version
    // change on row zero should select a cell array for copying.
    for trial in 0..301 {
        // The mutation bumps row zero's version before timing begins;
        // no parser or VT write is charged to the snapshot budget.
        let byte = if trial % 2 == 0 { b'X' } else { b'Y' };
        term.advance(&[b'\x1b', b'[', b'H', byte]);
        let start = Instant::now();
        snapshot.capture(&term, 101)?;
        row.push(start.elapsed().as_secs_f64() * 1_000_000.0);
        black_box(snapshot.copied_count());
    }
    full.sort_by(f64::total_cmp);
    row.sort_by(f64::total_cmp);
    std::io::stdout()
        .write_all(
            format!(
                "snapshot_full_us={:.3} snapshot_row_us={:.3}\n",
                full[50], row[150]
            )
            .as_bytes(),
        )
        .map_err(|error| format!("snapshot benchmark output: {error}"))
}

/// Returns a failure if either copy path cannot be measured.
fn main() -> Result<(), String> {
    run()
}

//! Quiet-run observation of the opt-in write-watch path, not a new budget.
//! The trace repeats the two-row count proof without scrolling or losing watches.
//! Outside writes leave the boundary live; inside writes exercise its sticky
//! invalidation. ASCII and one-cell Unicode use the same cursor/write geometry.
//! Five chronological trials and at least eight MiB match throughput_bench's
//! existing sampling protocol. Setup, watch allocation and CSV I/O are untimed.
//! This measures the opted-in path separately from the corpus A/B: the latter
//! deliberately has no watchers and is compared with the pre-clock revision.
//! Repeating cursor-addressed writes keeps the source row alive, avoiding an
//! accidental measurement of the no-watch path after scrollback eviction.
use puzzle_vt::{RangeWatch, Terminal};
use std::{hint::black_box, time::Instant};

/// A sustained source boundary is either absent, untouched, or already invalidated.
#[derive(Clone, Copy)]
enum Mode {
    /// Ordinary printing selects the no-watch parser instantiation.
    None,
    /// A live boundary exists, but both printed cells lie outside it.
    Outside,
    /// Rewriting the boundary keeps its sticky invalidation observable.
    Inside,
}

/// Stable CSV vocabulary makes the measured cases independently reviewable.
impl Mode {
    /// Names distinguish valid live boundaries from sticky invalidated ones.
    fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Outside => "outside",
            Self::Inside => "inside",
        }
    }
}

/// Build the complete input outside the clock, preserving every CSI boundary.
fn input(mode: Mode, scalar: char) -> Vec<u8> {
    let unit = unit(mode, scalar);
    let mut bytes = Vec::new();
    // Repeat whole units, as the existing corpus probe repeats whole source files.
    while bytes.len() < 8 * 1024 * 1024 {
        bytes.extend_from_slice(unit.as_bytes());
    }
    bytes
}

/// One two-row overwrite distinguishes boundary from unrelated row mutations.
fn unit(mode: Mode, scalar: char) -> String {
    // A watch covers zero-based columns [2,4). CUP uses one-based columns,
    // so column 3 is inside and column 1 is outside the same retained row.
    let column = if matches!(mode, Mode::Inside) { 3 } else { 1 };
    format!("\x1b[1;{column}H{scalar}\x1b[2;1Hi")
}

/// One fixed two-row write geometry remains live throughout every measured slice.
fn terminal(mode: Mode) -> Result<(Terminal, Option<RangeWatch>), String> {
    // This finite microtrace neither fills history nor runs a display. Geometry
    // is identical across cases; only the optional single-row witness differs.
    let mut terminal = Terminal::new(12, 4, 64, 64 * 1024);
    terminal.advance(b"abcdefgh\r\nijklmnop\r\nqrstuvwx");
    let watch = if !matches!(mode, Mode::None) {
        let first = terminal
            .screen()
            .row(0)
            .ok_or("first source row absent")?
            .id;
        // Both endpoints lie on the first row. Writing the second row must not
        // invalidate the outside case by becoming an interior-row mutation.
        Some(
            terminal
                .watch_source((first, 2), (first, 4), 1)
                .ok_or("source watch absent")?,
        )
    } else {
        None
    };
    Ok((terminal, watch))
}

/// Output is chronological and includes every trial, never selected minima.
fn main() -> Result<(), String> {
    let path = std::env::args().nth(1).ok_or("expected output CSV path")?;
    let mut cases = Vec::new();
    // Allocate every source and owner before timing. Scalar order and trial
    // numbers stay in the CSV so a reviewer can see drift instead of a best run.
    for scalar in ['a', 'é'] {
        for mode in [Mode::None, Mode::Outside, Mode::Inside] {
            cases.push((scalar, mode, input(mode, scalar), terminal(mode)?));
        }
    }
    let mut csv = String::from("scalar,watch,sample,bytes,ms\n");
    for trial in 0..5 {
        for (scalar, mode, bytes, (terminal, watch)) in &mut cases {
            // Reuse owners across trials, retaining warmed cluster allocations.
            // The production slice remains 16 KiB. No policy constant is changed.
            let started = Instant::now();
            for chunk in bytes.chunks(16 * 1024) {
                terminal.advance(black_box(chunk));
            }
            let elapsed = started.elapsed().as_secs_f64() * 1000.0;
            // A timing labelled outside is invalid if its witness was lost or
            // changed. This functional check stays outside the parser clock.
            if let Some(watch) = watch {
                let state = terminal.source_watch(*watch).ok_or("watch disappeared")?;
                if state.changed != matches!(mode, Mode::Inside) {
                    return Err("watch trace invalidated the wrong interval".into());
                }
            }
            // Report after the parser clock, without per-scalar instrumentation.
            csv.push_str(&format!(
                "{scalar},{},{trial},{},{elapsed:.6}\n",
                mode.name(),
                bytes.len()
            ));
        }
    }
    std::fs::write(path, csv).map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "source_watch_bench/tests.rs"]
/// Case labels are functional claims, independent of quiet-machine timing.
mod tests;

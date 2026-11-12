//! Isolates row ownership movement from parser and rendering costs (D-51).
//!
//! Geometry, operation count and output path are supplied by the caller. A
//! complete history warm-up precedes every clock, so initial allocations never
//! masquerade as region movement. Every chronological sample is retained.
//! Row versions now share a document write clock even in this isolated probe.
//! It replaces local version increments; it is unrelated to the timing clock.
use puzzle_vt::{PackedColor, Row, TrackedPositions, ring::RowRing};
use std::{hint::black_box, time::Instant};

/// Measures top-origin history insertion and ordinary partial-region rotation.
fn measure(columns: usize, rows: usize, count: usize) -> String {
    // Record representation size alongside timings so a faster parser cannot
    // hide a remaining wide row-header move in this isolated ownership stage.
    let mut csv = format!(
        "row_header_bytes,{}\npath,columns,rows,history,sample,ns_per_scroll\n",
        size_of::<Row>()
    );
    for history in [0, 10_000] {
        // Empty and populated histories exercise both physical deque layouts.
        for top_origin in [true, false] {
            let mut ring = RowRing::new(columns, rows, history, 32 * 1024 * 1024);
            let mut tracked = TrackedPositions::default();
            let mut writes = puzzle_vt::WriteClock::default();
            // There are no anchors in this probe: the clock isolates row
            // movement rather than unrelated tracked-position reconciliation.
            // Fill and wrap history before measuring a recycled incoming row.
            for _ in 0..history + rows {
                ring.scroll_up(&mut tracked, PackedColor::DEFAULT, &mut writes);
            }
            for sample in 0..5 {
                // Each sample continues the same warmed ring, crossing its
                // physical wrap repeatedly instead of choosing a favorable band.
                let start = Instant::now();
                for _ in 0..count {
                    if top_origin {
                        // Preserve a fixed footer while the top band enters history.
                        ring.scroll_top_region_up(
                            rows - 2,
                            &mut tracked,
                            PackedColor::DEFAULT,
                            &mut writes,
                        );
                    } else {
                        // A band starting below row zero never enters history.
                        ring.scroll_region_up(
                            1,
                            rows - 1,
                            &mut tracked,
                            PackedColor::DEFAULT,
                            &mut writes,
                        );
                    }
                }
                let ns = start.elapsed().as_secs_f64() * 1e9 / count as f64;
                // Observe the owner after timing to prevent dead-code removal
                // without inserting a diagnostic operation into each scroll.
                black_box(&ring);
                let path = if top_origin { "top" } else { "region" };
                // Formatting follows the clock; the first sample is not discarded.
                csv.push_str(&format!(
                    "{path},{columns},{rows},{history},{sample},{ns:.6}\n"
                ));
            }
        }
    }
    csv
}

/// Explicit destinations keep machine-specific timings out of shipped assets.
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [columns, rows, count, output] = args.as_slice() else {
        return Err("expected COLUMNS ROWS SCROLL_COUNT OUTPUT".into());
    };
    let columns = columns.parse::<usize>().map_err(|_| "invalid columns")?;
    let rows = rows.parse::<usize>().map_err(|_| "invalid rows")?;
    let count = count.parse::<usize>().map_err(|_| "invalid count")?;
    // Inclusive margins need at least one fixed row and a two-row band.
    // Reject malformed probe arguments before any subtraction or division.
    if columns < 2 || rows < 3 || count == 0 {
        return Err("requires columns >=2, rows >=3 and positive scroll count".into());
    }
    std::fs::write(output, measure(columns, rows, count)).map_err(|e| e.to_string())
}

//! D-51 counts real work through every warmed full-width scrolling entry point.
use super::*;
use crate::Cell;
use crate::scroll_operations::{Counts, measure};

/// History is filled to the same line ceiling at both widths; byte-limit
/// correctness has independent tests. Empty means history disabled, not cold
/// history growing: allocating a never-before-used row legitimately writes cells.
fn prepared(columns: usize, rows: usize, history: usize) -> (RowRing, TrackedPositions) {
    let mut clock = crate::WriteClock::default();
    let mut ring = RowRing::new(columns, rows, history, usize::MAX);
    let mut tracked = TrackedPositions::default();
    for _ in 0..2 * history + rows + rows / 2 {
        // Give recycled rows real old content so a clear cannot be optimized
        // out of the witness merely because the rows were already blank.
        let Some(row) = ring.visible_mut(rows - 1) else {
            panic!("prepared grid must contain its last visible row");
        };
        row.cells.store(columns - 1, ink());
        ring.scroll_up(&mut tracked, PackedColor::DEFAULT, &mut clock);
    }
    assert_eq!(ring.scrollback_len(), history);
    (ring, tracked)
}

/// Full-width stored ink prevents an already-blank setup from masking a clear.
fn ink() -> Cell {
    let mut cell = Cell::erased(PackedColor::DEFAULT);
    cell.ch = u32::from('x');
    cell
}

/// All operations share the public ring paths used by LF, SU/SD and IL/DL.
fn scroll(ring: &mut RowRing, tracked: &mut TrackedPositions, path: usize) -> usize {
    let mut clock = crate::WriteClock::default();
    let rows = ring.visible_rows();
    let paper = PackedColor::rgb(27, 39, 51);
    match path {
        0 => {
            ring.scroll_up(tracked, paper, &mut clock);
            rows
        }
        1 => {
            ring.scroll_top_region_up(rows - 2, tracked, paper, &mut clock);
            rows - 1
        }
        2 => {
            ring.scroll_region_up(1, rows - 1, tracked, paper, &mut clock);
            rows - 1
        }
        3 => {
            ring.scroll_region_down(1, rows - 1, tracked, paper, &mut clock);
            rows - 1
        }
        4 => {
            ring.scroll_region(1, rows - 1, rows / 2, true, tracked, paper, &mut clock);
            rows - 1
        }
        5 => {
            ring.scroll_region(1, rows - 1, rows / 2, false, tracked, paper, &mut clock);
            rows - 1
        }
        6 => {
            ring.scroll_region(0, rows - 1, 1, true, tracked, paper, &mut clock);
            rows
        }
        7 => {
            ring.scroll_region(0, rows - 1, 1, false, tracked, paper, &mut clock);
            rows
        }
        _ => unreachable!(),
    }
}

/// Each width observes the same sequence, including physical deque wrap.
fn observations(columns: usize, rows: usize, history: usize, path: usize) -> Vec<(usize, usize)> {
    let mut clock = crate::WriteClock::default();
    let (mut ring, mut tracked) = prepared(columns, rows, history);
    let mut observations = Vec::new();
    for _ in 0..2 * rows {
        // Advance the physical deque head between samples. Repeating a partial
        // rotation alone never wraps its band, even though logical rows move.
        ring.scroll_up(&mut tracked, PackedColor::DEFAULT, &mut clock);
        // The next scroll must erase stored ink, not only another implicit row.
        for row in &mut ring.rows {
            row.cells.store(columns - 1, ink());
        }
        let mut height = 0;
        let counts = measure(|| height = scroll(&mut ring, &mut tracked, path));
        assert_eq!(
            (counts.written, counts.cleared),
            (0, 0),
            "{columns}x{rows}/{history}/{path}: {counts:?}"
        );
        assert_eq!(
            counts.block_lookups, 0,
            "ordinary scroll resolved text blocks"
        );
        if path == 0 {
            assert_eq!(
                counts.handles.len(),
                1,
                "one actual push of the recycled owner"
            );
        }
        assert!(counts.handles.len() <= height, "{counts:?}");
        assert!(
            counts.exchanges <= height,
            "wrapped movement must also remain linear: {counts:?}"
        );
        observations.push((counts.handles.len(), counts.exchanges));
    }
    // Require the wrapped cycle itself, not merely a deque that wrapped during setup.
    if (2..=5).contains(&path) || (history > 0 && path >= 6) {
        assert!(
            observations.iter().any(|(_, exchanges)| *exchanges > 0),
            "fixture never measured a physically wrapped region"
        );
    }
    observations
}

/// Zero cell work, bounded row owners/exchanges, and exact equality at both widths.
#[test]
fn scroll_counts_are_zero_cells_bounded_handles_and_width_independent() {
    assert!(size_of::<RowHandle>() <= 8);
    let mut csv = String::from(
        "columns,rows,history,path,scroll,cells_written,cells_cleared,distinct_handles,exchanges\n",
    );
    for rows in [40, 63] {
        for history in [0, 10_000] {
            for path in 0..8 {
                let narrow = observations(120, rows, history, path);
                let wide = observations(282, rows, history, path);
                assert_eq!(
                    narrow, wide,
                    "width changed operations for rows={rows}, history={history}, path={path}"
                );
                for (columns, samples) in [(120, narrow), (282, wide)] {
                    for (scroll, (handles, exchanges)) in samples.into_iter().enumerate() {
                        csv.push_str(&format!(
                            "{columns},{rows},{history},{path},{scroll},0,0,{handles},{exchanges}\n"
                        ));
                    }
                }
            }
        }
    }
    // Optional evidence output is test instrumentation, absent from product code.
    if let Some(path) = std::env::var_os("PUZZLE_SCROLL_COUNT_REPORT")
        && let Err(error) = std::fs::write(path, csv)
    {
        panic!("could not write the requested scroll-count evidence: {error}");
    }
}

/// A reintroduced eager recycle must fail the zero-cell rule, not evade counting.
#[test]
fn scroll_counter_detects_eager_clearing_and_cell_copying() {
    let mut clock = crate::WriteClock::default();
    let (mut ring, mut tracked) = prepared(120, 40, 0);
    ring.make_rows_eager();
    let eager = measure(|| ring.scroll_up(&mut tracked, PackedColor::DEFAULT, &mut clock));
    assert_eq!(eager.cleared, 120);
    // A cell-copy regression also reaches the observer, even without a fill.
    let copied = measure(|| {
        let Some(row) = ring.visible(0) else {
            panic!("prepared grid must contain its first visible row");
        };
        let _copy = row.cells.clone();
    });
    assert_eq!(copied.written, 120);
    assert_ne!(eager, Counts::default());
}

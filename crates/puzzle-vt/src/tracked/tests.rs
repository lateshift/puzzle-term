//! Generation and ownership regressions for stable terminal markers.
//!
//! A search can be replaced repeatedly. Releasing its handles must keep
//! the registry bounded and prevent an old marker from resolving a new row.
#![allow(
    clippy::unwrap_used,
    reason = "tests assert fixture construction and exact worker results"
)]
use super::*;

/// Recycling a slot leaves copied handles Gone even when content is live.
#[test]
fn released_slot_is_reused_without_resurrecting_old_handle() {
    let mut positions = TrackedPositions::default();
    let old = positions.track(LineId(1), 2);
    positions.release(old);
    let fresh = positions.track(LineId(9), 3);
    assert_eq!(positions.positions.len(), 1);
    assert_eq!(positions.get(old), PositionState::Gone);
    assert_eq!(
        positions.get(fresh),
        PositionState::Present {
            line: LineId(9),
            column: 3
        }
    );
}

/// Independent screen rings reuse numeric IDs; only their own anchors mutate.
#[test]
fn inactive_screen_anchors_survive_eviction_and_reflow() {
    let mut positions = TrackedPositions::default();
    let primary = positions.track(LineId(1), 2);
    positions.set_screen(crate::terminal::ActiveScreen::Alternate);
    let alternate = positions.track(LineId(1), 3);
    positions.evict(LineId(1));
    assert_eq!(positions.get(alternate), PositionState::Gone);
    assert_eq!(
        positions.get(primary),
        PositionState::Present {
            line: LineId(1),
            column: 2
        }
    );
    let alternate = positions.track(LineId(2), 1);
    positions.set_screen(crate::terminal::ActiveScreen::Primary);
    positions.remap(|_, column| PositionState::Present {
        line: LineId(9),
        column,
    });
    assert_eq!(
        positions.get(primary),
        PositionState::Present {
            line: LineId(9),
            column: 2
        }
    );
    assert_eq!(
        positions.get(alternate),
        PositionState::Present {
            line: LineId(2),
            column: 1
        }
    );
}

/// Terminal resize visits both rings while retaining each screen's anchor domain.
#[test]
fn terminal_resize_and_alternate_clear_keep_primary_anchor() {
    let mut terminal = crate::Terminal::new(8, 2, 4, 4096);
    terminal.advance(b"abcdefghij");
    let primary = terminal
        .tracked
        .track(terminal.primary.row(1).unwrap().id, 1);
    terminal.advance(b"\x1b[?1049h");
    let alternate = terminal
        .tracked
        .track(terminal.alternate.row(0).unwrap().id, 1);
    terminal.resize(4, 2);
    assert!(matches!(
        terminal.tracked.get(primary),
        PositionState::Present { column: 1, .. }
    ));
    assert!(matches!(
        terminal.tracked.get(alternate),
        PositionState::Present { column: 1, .. }
    ));
    terminal.advance(b"\x1b[2J");
    assert_eq!(terminal.image_clear_epochs(), [0, 2]);
}

/// Image/search owners can leave large retired capacity; text scrolling must not visit it.
#[test]
fn retired_anchors_add_zero_work_to_image_free_scrolls() {
    for columns in [120, 282] {
        for rows in [40, 63] {
            for history in [false, true] {
                for region in [None, Some((1, rows - 1)), Some((0, rows - 2))] {
                    let mut fresh = scroll_fixture(columns, rows, history, region);
                    let mut retired = scroll_fixture(columns, rows, history, region);
                    let line = retired.screen().row(0).unwrap().id;
                    let handles: Vec<_> = (0..129)
                        .map(|column| retired.tracked.track(line, column))
                        .collect();
                    for handle in handles {
                        retired.tracked.release(handle);
                    }
                    let a = crate::scroll_operations::measure(|| fresh.advance(b"\n"));
                    let b = crate::scroll_operations::measure(|| retired.advance(b"\n"));
                    assert_eq!(a.anchors_examined, 0);
                    assert_eq!(b.anchors_examined, 0, "retired capacity is not live work");
                    assert_eq!(
                        (a.written, a.cleared, a.handles.len(), a.exchanges),
                        (b.written, b.cleared, b.handles.len(), b.exchanges)
                    );
                    // A region starting at row zero retains history, so its first
                    // scroll may allocate a new row exactly as in the fresh terminal.
                    if history || region.is_some_and(|(top, _)| top > 0) {
                        assert_eq!(
                            a.written + a.cleared,
                            0,
                            "{columns}x{rows} history={history} region={region:?} {a:?}"
                        );
                    }
                }
            }
        }
    }
}

/// Dense-list swaps cannot invalidate unrelated handles or resurrect Gone positions.
#[test]
fn live_anchor_eviction_and_reflow_preserve_handles_without_allocating() {
    let mut positions = TrackedPositions::default();
    let first = positions.track(LineId(1), 0);
    let second = positions.track(LineId(2), 1);
    let third = positions.track(LineId(3), 2);
    positions.release(second);
    let measured = allocation_counter::measure(|| positions.evict(LineId(1)));
    assert_eq!(measured.count_total, 0);
    assert_eq!(positions.get(first), PositionState::Gone);
    let counts = crate::scroll_operations::measure(|| positions.evict(LineId(8)));
    assert_eq!(counts.anchors_examined, 1);
    positions.remap(|_, _| PositionState::Gone);
    assert_eq!(positions.get(third), PositionState::Gone);
    let counts = crate::scroll_operations::measure(|| positions.evict(LineId(3)));
    assert_eq!(counts.anchors_examined, 0);
    positions.release(first);
    positions.release(third);
    let replacement = positions.track(LineId(9), 4);
    assert_eq!(positions.get(third), PositionState::Gone);
    let mut snapshot = TrackedPositions::default();
    snapshot.snapshot_from(&positions);
    let measured = allocation_counter::measure(|| snapshot.snapshot_from(&positions));
    assert_eq!(measured.count_total, 0);
    assert_eq!(snapshot.get(replacement), positions.get(replacement));
}

/// Constructs identical content without requiring a clone of parser state.
fn scroll_fixture(
    columns: usize,
    rows: usize,
    history: bool,
    region: Option<(usize, usize)>,
) -> crate::Terminal {
    let mut terminal = crate::Terminal::new(columns, rows, rows, usize::MAX);
    if history {
        terminal.advance(&vec![b'\n'; rows * 3]);
    }
    if let Some((top, bottom)) = region {
        terminal
            .advance(format!("\x1b[{};{}r\x1b[{};1H", top + 1, bottom + 1, bottom + 1).as_bytes());
    }
    terminal
}

/// Owner-only positions follow the same transforms without publication copies.
#[path = "tests/private.rs"]
mod private;

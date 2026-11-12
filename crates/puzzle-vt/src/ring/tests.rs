//! Scrollback limits and retained-line identity regressions.
use super::*;
use crate::{row::CellExtra, tracked::PositionState};

#[test]
fn tracked_position_survives_then_becomes_gone() {
    let mut clock = crate::WriteClock::default();
    let mut ring = RowRing::new(4, 2, 2, RowRing::row_storage_bytes(4) * 2);
    let mut tracked = TrackedPositions::default();
    let Some(first) = ring.visible(0).map(|row| row.id) else {
        panic!("initial row missing")
    };
    let marker = tracked.track(first, 1);
    ring.scroll_up(&mut tracked, PackedColor::DEFAULT, &mut clock);
    assert_eq!(
        tracked.get(marker),
        PositionState::Present {
            line: first,
            column: 1
        }
    );
    for _ in 0..8 {
        ring.scroll_up(&mut tracked, PackedColor::DEFAULT, &mut clock);
    }
    assert_eq!(tracked.get(marker), PositionState::Gone);
}

#[test]
fn line_ids_never_repeat() {
    let mut clock = crate::WriteClock::default();
    let mut ring = RowRing::new(4, 2, 2, RowRing::row_storage_bytes(4) * 2);
    let mut tracked = TrackedPositions::default();
    let mut ids = Vec::new();
    for _ in 0..8 {
        ring.scroll_up(&mut tracked, PackedColor::DEFAULT, &mut clock);
        let Some(id) = ring.newest_id() else {
            panic!("new row missing")
        };
        ids.push(id);
    }
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn byte_limit_restricts_history_independently_of_line_limit() {
    let mut clock = crate::WriteClock::default();
    let mut ring = RowRing::new(10, 2, 100, RowRing::row_storage_bytes(10) * 2);
    let mut tracked = TrackedPositions::default();
    for _ in 0..8 {
        ring.scroll_up(&mut tracked, PackedColor::DEFAULT, &mut clock);
    }
    assert_eq!(ring.scrollback_len(), 2);
    assert!(ring.history_bytes() <= RowRing::row_storage_bytes(10) * 2);
}

#[test]
fn warm_scroll_reuses_the_oldest_cell_allocation() {
    let mut clock = crate::WriteClock::default();
    let mut ring = RowRing::new(10, 2, 1, RowRing::row_storage_bytes(10));
    let mut tracked = TrackedPositions::default();
    ring.scroll_up(&mut tracked, PackedColor::DEFAULT, &mut clock);
    let Some(pointer) = ring.rows.front().map(|row| row.cells.as_ptr()) else {
        panic!("old row missing")
    };
    ring.scroll_up(&mut tracked, PackedColor::DEFAULT, &mut clock);
    // Oldest visible line rotates to bottom when the one-line history fills.
    assert_eq!(ring.visible(1).map(|row| row.cells.as_ptr()), Some(pointer));
    let allocations = allocation_counter::measure(|| {
        for _ in 0..16 {
            ring.scroll_up(&mut tracked, PackedColor::DEFAULT, &mut clock);
        }
    });
    assert_eq!(allocations.count_total, 0);
}

/// A partial scroll moves only pointer-sized owners, preserving all surviving row addresses.
#[test]
fn region_handles_keep_survivors_at_stable_addresses() {
    let mut clock = crate::WriteClock::default();
    assert!(
        size_of::<RowHandle>() <= 8,
        "D-51 maximum bytes per row move"
    );
    for columns in [120, 282] {
        for rows in [40, 63] {
            let mut ring = RowRing::new(columns, rows, 3, 32 * 1024 * 1024);
            let mut tracked = TrackedPositions::default();
            for _ in 0..rows + 3 {
                ring.scroll_up(&mut tracked, PackedColor::DEFAULT, &mut clock);
            }
            let before: Vec<_> = ring
                .iter()
                .map(|row| (row.id, std::ptr::from_ref(row)))
                .collect();
            let allocations = allocation_counter::measure(|| {
                ring.scroll_region_up(1, rows - 1, &mut tracked, PackedColor::DEFAULT, &mut clock);
                ring.scroll_top_region_up(rows - 2, &mut tracked, PackedColor::DEFAULT, &mut clock);
            });
            assert_eq!(allocations.count_total, 0);
            for row in ring.iter() {
                if let Some((_, pointer)) = before.iter().find(|(id, _)| *id == row.id) {
                    assert_eq!(*pointer, std::ptr::from_ref(row));
                }
            }
        }
    }
}

#[test]
fn top_region_accounts_for_outgoing_cluster_bytes() {
    let mut clock = crate::WriteClock::default();
    // Include the newly boxed row header; the witness still admits exactly two rows.
    let row_bytes = RowRing::row_storage_bytes(4);
    let mut ring = RowRing::new(4, 3, 2, row_bytes * 2);
    let mut tracked = TrackedPositions::default();
    // The cluster crosses into history on SU, where its heap bytes make
    // two otherwise legal full-width rows exceed the byte ceiling.
    if let Some(row) = ring.visible_mut(0) {
        row.set_extra(
            CellExtra {
                column: 0,
                cluster: Some("á".repeat(16)),
                underline_color: None,
                link_id: None,
            },
            &mut clock,
        );
    }
    ring.scroll_top_region_up(1, &mut tracked, PackedColor::DEFAULT, &mut clock);
    assert!(ring.history_bytes() > row_bytes);
    ring.scroll_top_region_up(1, &mut tracked, PackedColor::DEFAULT, &mut clock);
    assert!(ring.history_bytes() <= row_bytes * 2);
}

/// Batched SU/SD must preserve footer/history and exact line identities across wrap.
#[test]
fn region_counts_match_single_steps_without_allocation() {
    // Cover no-history whole-grid rotation, physical fragments and wrapped history.
    for history in [0, 3] {
        for (top, bottom) in [(0, 7), (1, 6), (0, 3), (4, 7)] {
            for upward in [true, false] {
                for count in 0..=9 {
                    compare_region(history, top, bottom, count, upward);
                }
            }
        }
    }
}

/// Compare all boundary/count cases through the previous single-step contract.
fn compare_region(history: usize, top: usize, bottom: usize, count: usize, upward: bool) {
    let mut clock = crate::WriteClock::default();
    let mut ring = RowRing::new(4, 8, history, 1024);
    let mut tracked = TrackedPositions::default();
    for _ in 0..9 {
        ring.scroll_up(&mut tracked, PackedColor::DEFAULT, &mut clock);
    }
    for index in 0..8 {
        let Some(row) = ring.visible_mut(index) else {
            panic!("visible row absent")
        };
        row.cells[0].ch = b'a' as u32 + index as u32;
    }
    let mut sequential = ring.clone();
    let mut old_tracked = tracked.clone();
    let Some(id) = ring
        .visible(if upward { top } else { bottom })
        .map(|row| row.id)
    else {
        panic!("tracked row absent")
    };
    let gone = tracked.track(id, 0);
    let old_gone = old_tracked.track(id, 0);
    // Counts larger than the region clear once, rather than generate extra IDs.
    for _ in 0..count.min(bottom - top + 1) {
        sequential.scroll_region(
            top,
            bottom,
            1,
            upward,
            &mut old_tracked,
            PackedColor::DEFAULT,
            &mut clock,
        );
    }
    let allocations = allocation_counter::measure(|| {
        ring.scroll_region(
            top,
            bottom,
            count,
            upward,
            &mut tracked,
            PackedColor::DEFAULT,
            &mut clock,
        );
    });
    assert_eq!(allocations.count_total, 0);
    assert_eq!(tracked.get(gone), old_tracked.get(old_gone));
    for (actual, expected) in ring.iter().zip(sequential.iter()) {
        assert_eq!(actual.id, expected.id);
        assert_eq!(actual.cells, expected.cells);
    }
}

/// Byte-limited sparse history must recycle before allocating and keep exact accounting.
#[test]
fn sparse_byte_capacity_recycles_and_resize_recounts() {
    let mut clock = crate::WriteClock::default();
    let mut ring = RowRing::new(12, 3, 100, RowRing::row_storage_bytes(12) * 8);
    let mut tracked = TrackedPositions::default();
    for _ in 0..32 {
        sparse_scroll(&mut ring, &mut tracked);
        assert_history_bytes(&ring);
    }
    let before = ring.storage_growth();
    let allocation = allocation_counter::measure(|| {
        for _ in 0..64 {
            sparse_scroll(&mut ring, &mut tracked);
        }
    });
    assert_eq!(allocation.count_total, 0);
    assert_eq!(ring.storage_growth(), before);
    assert_history_bytes(&ring);
    ring.resize_reflow(6, 3, &mut tracked, None);
    assert_history_bytes(&ring);
    ring.resize_plain(10, 3, &mut tracked, &mut clock);
    assert_history_bytes(&ring);
    ring.set_limits(2, 500, &mut tracked);
    assert_history_bytes(&ring);
    ring.clear_scrollback(&mut tracked);
    assert_eq!(ring.history_bytes(), 0);
}

/// Reuse a real multi-scalar store on each scroll at the smaller byte ceiling.
fn sparse_scroll(ring: &mut RowRing, tracked: &mut TrackedPositions) {
    let mut clock = crate::WriteClock::default();
    let Some(row) = ring.visible_mut(0) else {
        panic!("top row absent")
    };
    let mut cluster = row.cluster_storage(0);
    cluster.clear();
    cluster.push_str("a\u{0301}");
    row.cells[0].ch = crate::Cell::CLUSTER;
    row.set_extra(
        CellExtra {
            column: 0,
            cluster: Some(cluster),
            underline_color: None,
            link_id: None,
        },
        &mut clock,
    );
    ring.scroll_up(tracked, PackedColor::DEFAULT, &mut clock);
}

/// Independently sum retained row allocations after every destructive boundary.
fn assert_history_bytes(ring: &RowRing) {
    let exact: usize = ring
        .iter()
        .take(ring.scrollback_len())
        .map(RowRing::retained_row_bytes)
        .sum();
    assert_eq!(ring.history_bytes(), exact);
    assert!(exact <= ring.scrollback_bytes);
}

/// RIS/ED3 retire identities, but repeated output can reuse the bounded row storage.
#[test]
fn cleared_history_reuses_storage_without_allocations() {
    let mut clock = crate::WriteClock::default();
    let mut ring = RowRing::new(282, 63, 10_000, 32 * 1024 * 1024);
    let mut tracked = TrackedPositions::default();
    for _ in 0..10_000 {
        ring.scroll_up(&mut tracked, PackedColor::DEFAULT, &mut clock);
    }
    let Some(oldest) = ring.oldest_id() else {
        panic!("history absent");
    };
    let marker = tracked.track(oldest, 0);
    let growth = ring.storage_growth();
    let allocations = allocation_counter::measure(|| {
        for _ in 0..3 {
            ring.clear_scrollback(&mut tracked);
            assert_eq!(ring.scrollback_len(), 0);
            for _ in 0..10_000 {
                ring.scroll_up(&mut tracked, PackedColor::DEFAULT, &mut clock);
            }
        }
    });
    assert_eq!(tracked.get(marker), PositionState::Gone);
    assert_eq!(ring.storage_growth(), growth);
    assert_eq!(allocations.count_total, 0);
}

/// Inactive allocations share live limits and cannot resurrect retired history on resize.
#[test]
fn inactive_history_obeys_limits_geometry_and_bce() {
    let mut clock = crate::WriteClock::default();
    let mut ring = RowRing::new(120, 40, 100, 32 * 1024 * 1024);
    let mut tracked = TrackedPositions::default();
    for _ in 0..100 {
        ring.scroll_up(&mut tracked, PackedColor::DEFAULT, &mut clock);
    }
    ring.clear_scrollback(&mut tracked);
    assert_eq!(ring.scrollback_len(), 0);
    assert!(ring.allocated_history_bytes() <= 32 * 1024 * 1024);
    let blue = PackedColor::palette(4);
    ring.scroll_up(&mut tracked, blue, &mut clock);
    let Some(bottom) = ring.visible(39) else {
        panic!("visible bottom absent");
    };
    assert!(bottom.cells.iter().all(|cell| cell.bg == blue));
    ring.set_limits(2, RowRing::row_storage_bytes(120) * 2, &mut tracked);
    assert!(ring.allocated_history_bytes() <= RowRing::row_storage_bytes(120) * 2);
    assert!(ring.spares.len() + ring.scrollback_len() <= 2);
    ring.resize_plain(282, 40, &mut tracked, &mut clock);
    assert!(ring.spares.is_empty());
    assert!(ring.iter().all(|row| row.cells.len() == 282));
}

/// Deterministic structural scroll acceptance replaces timing comparisons.
#[path = "tests/operation_counts.rs"]
mod operation_counts;

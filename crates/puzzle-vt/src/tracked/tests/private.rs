//! Owner-only source positions must never add private copies to a public frame.
use super::*;

#[test]
fn private_positions_follow_eviction_reflow_and_screen_domains() {
    let mut root = TrackedPositions::default();
    let published = root.track(LineId(1), 1);
    let private = root.track_private(LineId(1), 2);
    root.set_screen(crate::terminal::ActiveScreen::Alternate);
    let alternate = root.track_private(LineId(1), 3);
    root.evict(LineId(1));
    assert_eq!(root.get_private(alternate), PositionState::Gone);
    root.set_screen(crate::terminal::ActiveScreen::Primary);
    root.remap(|_, column| PositionState::Present {
        line: LineId(7),
        column,
    });
    assert_eq!(
        root.get(published),
        PositionState::Present {
            line: LineId(7),
            column: 1
        }
    );
    assert_eq!(
        root.get_private(private),
        PositionState::Present {
            line: LineId(7),
            column: 2
        }
    );
    assert_eq!(root.get_private(alternate), PositionState::Gone);
    root.release_private(alternate);
    root.release_private(private);
    assert_eq!(
        root.get(published),
        PositionState::Present {
            line: LineId(7),
            column: 1
        }
    );
}

#[test]
fn public_clone_and_rotating_snapshot_never_copy_private_positions_or_capacity() {
    let mut root = TrackedPositions::default();
    let public = root.track(LineId(1), 1);
    let handles: Vec<_> = (0..1024)
        .map(|column| root.track_private(LineId(1), column))
        .collect();
    assert!(root.private_storage_bytes() > 0);
    let clone = root.clone();
    assert_eq!(clone.get(public), root.get(public));
    assert_eq!(clone.get_private(handles[0]), PositionState::Gone);
    assert_eq!(clone.private_storage_bytes(), 0);
    let mut slot = TrackedPositions::default();
    slot.snapshot_from(&root);
    let measured = allocation_counter::measure(|| slot.snapshot_from(&root));
    assert_eq!(measured.count_total, 0);
    assert_eq!(slot.private_storage_bytes(), 0);
    assert_eq!(slot.positions.capacity(), clone.positions.capacity());
}

#[test]
fn fully_retired_private_pool_is_reused_without_resurrecting_a_copied_handle() {
    let mut root = TrackedPositions::default();
    let old = root.track_private(LineId(1), 1);
    let charged = root.private_storage_bytes();
    let measured = allocation_counter::measure(|| root.release_private(old));
    assert_eq!(measured.count_total, 0);
    assert_eq!(root.private_storage_bytes(), charged);
    let next = root.track_private(LineId(2), 2);
    assert_eq!(root.get_private(old), PositionState::Gone);
    assert_eq!(
        root.get_private(next),
        PositionState::Present {
            line: LineId(2),
            column: 2
        }
    );
    root.release_private(old);
    assert_eq!(
        root.get_private(next),
        PositionState::Present {
            line: LineId(2),
            column: 2
        }
    );
    root.release_private(next);
    let counts = crate::scroll_operations::measure(|| root.evict(LineId(2)));
    assert_eq!(counts.anchors_examined, 0);
}

#[test]
fn private_capacity_is_exactly_reported_and_absent_without_an_owner() {
    let mut root = TrackedPositions::default();
    assert_eq!(root.private_storage_bytes(), 0);
    let measured = allocation_counter::measure(|| {
        for column in 0..32 {
            root.track_private(LineId(1), column);
        }
    });
    let retained: usize = measured.bytes_current.try_into().unwrap();
    assert_eq!(root.private_storage_bytes(), retained);
    assert!(retained > 0);
}

#[test]
fn exhausted_slot_generation_retires_without_wrapping_or_aliasing() {
    let mut root = TrackedPositions::default();
    let old = root.track(LineId(1), 1);
    root.positions[old.index].generation = u64::MAX;
    let old = TrackedPos {
        generation: u64::MAX,
        ..old
    };
    root.release(old);
    let next = root.track(LineId(2), 2);
    assert_ne!(old.index, next.index);
    assert_eq!(root.get(old), PositionState::Gone);
    assert_eq!(
        root.get(next),
        PositionState::Present {
            line: LineId(2),
            column: 2
        }
    );
}

#[test]
fn owner_only_markers_add_zero_allocations_to_a_publication() {
    let mut root = TrackedPositions::default();
    for column in 0..1024 {
        root.track_private(LineId(1), column);
    }
    let measured = allocation_counter::measure(|| {
        let clone = root.clone();
        assert_eq!(clone.private_storage_bytes(), 0);
        assert!(clone.positions.is_empty());
    });
    assert_eq!(measured.count_total, 0);
}

//! Hidden row ranges preserve exact source identity without visiting their contents.
use super::*;

#[test]
fn merged_omissions_map_without_visiting_hidden_rows() {
    let mut projection = RowProjection::default();
    assert!(projection.rebuild(30_000, [2..20_000, 3..10, 20_000..20_002]));
    assert_eq!(projection.len(), 10_000);
    assert_eq!(projection.source_row(0), Some(0));
    assert_eq!(projection.source_row(1), Some(1));
    assert_eq!(projection.source_row(2), Some(20_002));
    assert_eq!(projection.source_row(9_999), Some(29_999));
    assert_eq!(projection.source_row(10_000), None);
    assert_eq!(projection.display_row(20_001), None);
    assert_eq!(projection.display_row(20_002), Some(2));
    assert_eq!(projection.display_anchor(19_999), Some(1));
    assert!(!projection.rebuild(30_000, std::iter::once(2..20_002)));
}

#[test]
fn two_omissions_match_exhaustive_short_document_models() {
    for total in 0..10 {
        for a in 0..=total {
            for b in a..=total {
                for c in 0..=total {
                    for d in c..=total {
                        let mut projection = RowProjection::default();
                        projection.rebuild(total, [a..b, c..d]);
                        let visible: Vec<_> = (0..total)
                            .filter(|row| !(a..b).contains(row) && !(c..d).contains(row))
                            .collect();
                        assert_eq!(projection.len(), visible.len());
                        for (index, source) in visible.iter().enumerate() {
                            assert_eq!(projection.source_row(index), Some(*source));
                            assert_eq!(projection.display_row(*source), Some(index));
                        }
                        for source in 0..total {
                            if !visible.contains(&source) {
                                assert_eq!(projection.display_row(source), None);
                                let before = visible
                                    .iter()
                                    .position(|row| *row > source)
                                    .unwrap_or(visible.len())
                                    .checked_sub(1);
                                assert_eq!(projection.display_anchor(source), before);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn empty_mapping_clamps_ends_and_warm_ranges_reuse_capacity() {
    let mut projection = RowProjection::default();
    assert!(!projection.rebuild(0, std::iter::once(1..usize::MAX)));
    assert_eq!(projection.allocated_bytes(), 0);
    assert!(projection.is_empty());
    assert_eq!(projection.source_row(0), None);
    projection.rebuild(30_000, [2..20_000, 20_001..20_010]);
    projection.rebuild(30_000, [2..20_000, 20_001..20_010]);
    let allocation = allocation_counter::measure(|| {
        for _ in 0..100 {
            projection.rebuild(30_000, std::iter::once(2..20_000));
            projection.rebuild(30_000, [2..20_000, 20_001..20_010]);
        }
    });
    assert_eq!(allocation.count_total, 0);
    assert!(projection.allocated_bytes() > 0);
    assert!(projection.rebuild(30_000, []));
    assert_eq!(projection.len(), 30_000);
    assert!(!projection.is_empty());
}

#[test]
fn projection_allocation_is_bounded_by_live_intervals_not_source_rows() {
    for count in 1..=256 {
        let mut projection = RowProjection::default();
        for _ in 0..3 {
            projection.rebuild(1_000_000, (0..count).map(|i| 4 * i + 1..4 * i + 3));
            assert!(projection.allocated_bytes() <= RowProjection::storage_bound(count));
        }
    }
}

#[test]
fn marked_projection_retains_one_blank_slot_for_nested_and_single_row_folds() {
    let mut map = RowProjection::default();
    assert!(map.rebuild_marked(8, [1..5, 2..4, 6..7]));
    assert_eq!(map.len(), 5);
    assert_eq!(
        (0..map.len())
            .map(|row| map.source_row(row))
            .collect::<Vec<_>>(),
        [Some(0), Some(1), Some(5), Some(6), Some(7)]
    );
    assert!(map.is_marker(1));
    assert!(map.is_marker(6));
    assert!(!map.is_marker(5));
    assert!(!map.rebuild_marked(8, [1..5, 2..4, 6..7]));
    assert!(map.rebuild_marked(8, []));
    assert_eq!(map.len(), 8);
    assert!(!map.is_marker(1));
}

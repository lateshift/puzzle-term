//! Bulk BCE erasure preserves split-wide rules and retires rare metadata.
use super::*;

/// A span beginning in a spacer and ending at a lead must clear both outside partners.
#[test]
fn bulk_erase_retires_both_wide_borders_and_reuses_cluster_storage() {
    let mut clock = crate::WriteClock::default();
    let mut row = Row::new(LineId(1), 8);
    let mut leading = Cell::default();
    leading.ch = '漢' as u32;
    leading.flags = CellFlags::WIDE;
    let mut spacer = Cell::default();
    spacer.flags = CellFlags::WIDE_SPACER;
    for column in [1, 4] {
        row.put(column, leading, &mut clock);
        row.put(column + 1, spacer, &mut clock);
    }
    row.set_extra(
        CellExtra {
            column: 2,
            cluster: Some(String::with_capacity(32)),
            underline_color: None,
            link_id: Some(7),
        },
        &mut clock,
    );
    let background = PackedColor::rgb(1, 2, 3);
    let before = row.version;
    let allocation = allocation_counter::measure(|| {
        row.erase_unprotected(2, 5, Cell::erased(background), &mut clock)
    });
    assert_eq!(allocation.count_total, 0);
    assert!(row.version != before);
    assert!(row.extras.is_empty());
    assert_eq!(row.cluster_spares.len(), 1);
    for column in 1..=5 {
        assert_eq!(row.cells[column], Cell::erased(background));
    }
    assert_eq!(row.cells[0], Cell::default());
    assert_eq!(row.cells[6], Cell::default());
}

/// Full replacement writes only the supplied cell, while preserving genuine BCE gaps.
#[test]
fn replacement_skips_redundant_fill_and_preserves_sparse_metadata_rules() {
    let mut clock = crate::WriteClock::default();
    let mut row = Row::new(LineId(1), 8);
    let paper = PackedColor::rgb(12, 34, 56);
    row.cells.fill(Cell::erased(paper));
    let mut cell = Cell::default();
    cell.ch = '漢' as u32;
    let counts = crate::scroll_operations::measure(|| row.put(0, cell, &mut clock));
    assert_eq!((counts.written, counts.cleared), (1, 0));
    let counts = crate::scroll_operations::measure(|| row.put(4, cell, &mut clock));
    assert_eq!((counts.written, counts.cleared), (1, 3));
    for column in 1..4 {
        assert_eq!(row.cells[column], Cell::erased(paper));
    }
    assert_eq!(row.cells[4], cell);
    assert_eq!(row.cells[5], Cell::erased(paper));
    row.set_extra(
        CellExtra {
            column: 4,
            cluster: Some("á".into()),
            underline_color: None,
            link_id: Some(7),
        },
        &mut clock,
    );
    let version = row.version;
    row.put(4, Cell::default(), &mut clock);
    assert!(row.extras.is_empty());
    assert_eq!(row.cluster_spares.len(), 1);
    assert_ne!(row.version, version);
}

//! Corrupted grid witnesses must fail even when the visible text looks plausible.
use super::*;
use crate::{LineId, Terminal};

/// An unwritten row exercises logical lazy-fill access rather than raw capacity.
fn blank() -> Row {
    Row::new(LineId(7), 4)
}

/// Both wide directions and mutually exclusive representations are enforced.
#[test]
fn detects_orphaned_and_conflicting_wide_halves() {
    for (column, flags) in [
        (0, CellFlags::WIDE_SPACER),
        (3, CellFlags::WIDE),
        (1, CellFlags::WIDE),
        (1, CellFlags::WIDE | CellFlags::WIDE_SPACER),
        (1, CellFlags::WIDE | CellFlags::MULTICELL),
    ] {
        let mut row = blank();
        row.cells[column].flags = flags;
        assert!(check_row(&row).is_err());
    }
    let mut row = blank();
    row.cells[1].flags = CellFlags::WIDE;
    row.cells[2].flags = CellFlags::WIDE_SPACER;
    assert_eq!(check_row(&row), Ok(()));
}

/// A sentinel, extra, flag and cluster text must name the same owning cell.
#[test]
fn detects_missing_and_stale_cluster_ownership() {
    let mut clock = crate::WriteClock::default();
    let mut row = blank();
    row.cells[1].ch = Cell::CLUSTER;
    assert!(check_row(&row).is_err());
    row.set_extra(
        CellExtra {
            column: 1,
            cluster: Some("e\u{301}".into()),
            underline_color: None,
            link_id: None,
        },
        &mut clock,
    );
    assert_eq!(check_row(&row), Ok(()));
    row.cells[1].flags.remove(CellFlags::HAS_EXTRAS);
    assert!(check_row(&row).is_err());
    row.cells[1].flags.insert(CellFlags::HAS_EXTRAS);
    row.cells[1].ch = u32::from('x');
    assert!(check_row(&row).is_err());
    row.cells[1].ch = Cell::CLUSTER;
    row.extras[0].cluster = None;
    assert!(check_row(&row).is_err());
    row.extras[0].cluster = Some(String::new());
    assert!(check_row(&row).is_err());
    row.extras.clear();
    row.cells[1].ch = 0;
    assert!(check_row(&row).is_err());
    // Even a paired spacer cannot own text; the leading cell owns its cluster.
    row.cells[0].flags = CellFlags::WIDE;
    row.cells[1].flags = CellFlags::WIDE_SPACER;
    row.cells[1].ch = Cell::CLUSTER;
    row.set_extra(
        CellExtra {
            column: 1,
            cluster: Some("e\u{301}".into()),
            underline_color: None,
            link_id: None,
        },
        &mut clock,
    );
    assert!(check_row(&row).is_err());
}

/// Decorated ordinary cells are legal; duplicate/outside sparse references are not.
#[test]
fn validates_decoration_entries_and_sparse_order() {
    let mut clock = crate::WriteClock::default();
    let mut row = blank();
    let extra = CellExtra {
        column: 1,
        cluster: None,
        underline_color: Some(crate::PackedColor::palette(2)),
        link_id: Some(1),
    };
    row.set_extra(extra.clone(), &mut clock);
    assert_eq!(check_row(&row), Ok(()));
    row.extras.push(extra.clone());
    assert!(check_row(&row).is_err());
    row.extras.pop();
    row.cells[1].flags.remove(CellFlags::HAS_EXTRAS);
    row.extras[0].column = row.cells.len();
    assert!(check_row(&row).is_err());
}

/// A valid mixed document checks history and both layout types without allocating.
#[test]
fn successful_grid_checks_allocate_nothing() {
    let mut term = Terminal::new(12, 4, 64, 65536);
    for _ in 0..16 {
        term.advance("e\u{301}界\r\n".as_bytes());
    }
    term.advance(b"\x1b]66;s=1:w=2;block\x07");
    for screen in [&term.primary, &term.alternate] {
        assert_eq!(screen.ring.check_grid(), Ok(()));
    }
    let allocations = allocation_counter::measure(|| {
        for _ in 0..8 {
            assert_eq!(term.primary.ring.check_grid(), Ok(()));
            assert_eq!(term.alternate.ring.check_grid(), Ok(()));
        }
    });
    assert_eq!(allocations.count_total, 0);
    assert_eq!(allocations.bytes_total, 0);
}

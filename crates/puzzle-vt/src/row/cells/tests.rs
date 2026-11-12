//! Lazy rows are compared with the former eager storage policy over VT operations.
//! The oracle shares parser semantics but writes every recycled/erased column.
//! Random edits include wide text, sparse clusters, BCE, margins and reflow;
//! physical stale cells must never leak through any logical reader or clone.
use super::*;
use crate::{PackedColor, Terminal};
use proptest::prelude::*;

/// Recycling a cold row changes no physical cells and charges its original capacity.
#[test]
fn recycle_leaves_cold_cells_untouched_and_gap_uses_bce() {
    let mut clock = crate::WriteClock::default();
    let mut row = crate::Row::new(crate::LineId(0), 282);
    let mut ink = Cell::default();
    ink.ch = u32::from('X');
    row.cells.fill(ink);
    row.cells.materialize(282);
    let physical = row.cells.storage.clone();
    let bytes = row.retained_bytes();
    let background = PackedColor::rgb(21, 45, 78);
    let allocations =
        allocation_counter::measure(|| row.recycle(crate::LineId(1), background, &mut clock));
    assert_eq!(allocations.count_total, 0);
    assert_eq!(row.cells.storage, physical);
    assert_eq!(row.cells.written_extent(), 0);
    assert_eq!(row.retained_bytes(), bytes);
    assert!(
        row.cells
            .iter()
            .all(|cell| *cell == Cell::erased(background))
    );
    row.put(3, ink, &mut clock);
    assert_eq!(row.cells.written_extent(), 4);
    assert_eq!(row.cells[2], Cell::erased(background));
    assert_eq!(row.cells[4], Cell::erased(background));
    assert_eq!(row.cells.storage[281], ink, "untouched physical tail");
}

/// Warm copies reuse full-width storage but only write the authoritative prefix.
#[test]
fn snapshot_copy_never_materializes_or_copies_the_implicit_suffix() {
    let mut cells = RowCells::new(282);
    let mut target = cells.clone();
    cells[0].ch = u32::from('y');
    let retired = Cell::erased(PackedColor::rgb(1, 2, 3));
    target.storage.fill(retired);
    let allocations = allocation_counter::measure(|| target.clone_from(&cells));
    assert_eq!(allocations.count_total, 0);
    assert_eq!(target, cells);
    assert_eq!(target.storage[281], retired);
    assert_eq!(target[281], Cell::default());
}

/// Expanding a colored erased row preserves its old paper, adding default paper only at the edge.
#[test]
fn plain_resize_preserves_bce_tail_and_new_column_defaults() {
    let mut cells = RowCells::new(8);
    let colored = Cell::erased(PackedColor::rgb(8, 9, 10));
    cells.fill(colored);
    cells.resize(12, Cell::default());
    assert!(cells.iter().take(8).all(|cell| *cell == colored));
    assert!(cells.iter().skip(8).all(|cell| *cell == Cell::default()));
    cells.resize(4, Cell::default());
    cells.resize(6, Cell::default());
    assert_eq!(cells[3], colored);
    assert_eq!(cells[4], Cell::default());
}

/// An EL beyond the stored text must not write the untouched gap or retired tail.
#[test]
fn suffix_erase_with_same_background_keeps_the_gap_implicit() {
    let mut clock = crate::WriteClock::default();
    let mut row = crate::Row::new(crate::LineId(0), 282);
    row.cells[0].ch = u32::from('y');
    let physical = row.cells.storage.clone();
    row.erase_unprotected(120, 282, Cell::default(), &mut clock);
    assert_eq!(row.cells.written_extent(), 1);
    assert_eq!(row.cells.storage, physical);
    row.erase_unprotected(0, 282, Cell::default(), &mut clock);
    assert_eq!(row.cells.written_extent(), 0);
}

/// Each opcode is a real terminal operation rather than a representation mutation.
fn operation(term: &mut Terminal, op: u8, arg: u8) {
    let (columns, rows) = term.size();
    let count = usize::from(arg) % columns + 1;
    let bytes = match op % 14 {
        0 => format!("\x1b[{};{}H", usize::from(arg) % rows + 1, count),
        1 => format!(
            "\x1b[48;2;{};{};{}m",
            arg,
            arg.wrapping_add(45),
            arg.wrapping_add(90)
        ),
        2 => format!("\x1b[{}K", arg % 3),
        3 => format!("\x1b[{}J", arg % 4),
        4 => format!("\x1b[{count}X"),
        5 => format!("\x1b[{count}@"),
        6 => format!("\x1b[{count}P"),
        7 => format!("\x1b[{count}S"),
        8 => format!("\x1b[{count}T"),
        9 => format!("\x1b[2;{}r", rows.max(2)),
        10 => "\x1b[r\r\n".into(),
        11 => "a漢✌️\r\n".into(),
        12 => "\x1b[0mwords\r\n".into(),
        _ => {
            term.resize(2 + usize::from(arg) % 19, 2 + usize::from(arg) % 7);
            return;
        }
    };
    term.advance(bytes.as_bytes());
}

proptest! {
    /// Logical rows, copied rows, dump text and every sparse extra match the eager oracle.
    #[test]
    fn random_vt_edits_match_eager_rows(ops in prop::collection::vec((any::<u8>(), any::<u8>()), 1..200)) {
        let mut lazy = Terminal::new(12, 6, 12, 32 * 1024);
        let mut eager = Terminal::new(12, 6, 12, 32 * 1024);
        for (op, arg) in ops {
            eager.primary.ring.make_rows_eager();
            eager.alternate.ring.make_rows_eager();
            operation(&mut lazy, op, arg);
            operation(&mut eager, op, arg);
            prop_assert_eq!(lazy.size(), eager.size());
            prop_assert_eq!(lazy.cursor.column, eager.cursor.column);
            prop_assert_eq!(lazy.cursor.row, eager.cursor.row);
            let a: Vec<_> = lazy.screen().ring.iter().collect();
            let b: Vec<_> = eager.screen().ring.iter().collect();
            prop_assert_eq!(a.len(), b.len());
            for (left, right) in a.into_iter().zip(b) {
                prop_assert_eq!(left.id, right.id);
                prop_assert_eq!(left.flags, right.flags);
                prop_assert_eq!(&left.extras, &right.extras);
                prop_assert_eq!(&left.cells, &right.cells);
                // A snapshot is a distinct row owner; clone must preserve implicit fill too.
                prop_assert_eq!(left.cells.clone(), right.cells.clone());
                let text = |row: &crate::Row| row.cells.iter().filter_map(|c| char::from_u32(c.ch)).collect::<String>();
                prop_assert_eq!(text(left), text(right));
            }
        }
    }
}

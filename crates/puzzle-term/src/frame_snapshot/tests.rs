//! T1 snapshot regressions over real VT state and reused row buffers.
//!
//! Cursor movement and OSC palette changes can invalidate geometry without
//! changing printable cells, so these cases accompany an allocation probe
//! that repeatedly copies already warmed 240-column rows.
use super::*;

/// A later title notification must not relabel ink from an earlier publication.
#[test]
fn title_and_cells_share_the_publication_boundary() {
    let mut term = Terminal::new(8, 3, 0, 0);
    term.advance(b"old\x1b]2;running\x07");
    let mut snapshot = FrameSnapshot::default();
    assert!(snapshot.capture(&term, 0).is_ok());
    // The Session may finish a new screen and emit its title while the renderer
    // is still preparing the preceding slot. Neither value in that slot changes.
    term.advance(b"\x1b[Hnew\x1b]2;frozen\x07");
    assert_eq!(snapshot.title(), "running");
    assert_eq!(
        snapshot.retained_row(0).map(|row| row.cells[0].ch),
        Some(u32::from('o'))
    );
    assert!(snapshot.capture(&term, 0).is_ok());
    assert_eq!(snapshot.title(), "frozen");
    assert_eq!(
        snapshot.retained_row(0).map(|row| row.cells[0].ch),
        Some(u32::from('n'))
    );
    // Ordinary repeats reuse the title and row buffers without warm allocations.
    assert_eq!(
        allocation_counter::measure(|| {
            assert!(snapshot.capture(&term, 0).is_ok());
        })
        .count_total,
        0
    );
}

/// One edit copies one row; moving a cursor can copy its old and new rows.
#[test]
fn capture_only_changed_visible_rows() {
    let mut term = Terminal::new(8, 3, 0, 0);
    let mut snapshot = FrameSnapshot::default();
    assert!(snapshot.capture(&term, 0).is_ok());
    assert_eq!(snapshot.copied_count(), 3);
    assert!(snapshot.capture(&term, 0).is_ok());
    assert_eq!(snapshot.copied_count(), 0);
    term.advance(b"X");
    assert!(snapshot.capture(&term, 0).is_ok());
    assert_eq!(snapshot.copied_count(), 1);
    term.advance(b"\x1b]4;1;#123456\x07");
    assert!(snapshot.capture(&term, 0).is_ok());
    assert_eq!(snapshot.copied_count(), 3);
}

/// `clone_from` keeps the warmed cell vectors during a full-grid redraw.
#[test]
fn warm_full_snapshot_allocates_nothing() {
    let mut term = Terminal::new(240, 70, 0, 0);
    // Multi-scalar emoji carry sparse owned strings; derived clone_from used
    // to allocate per cluster even after the outer row capacity was warm.
    for row in 0..70 {
        term.advance(format!("\x1b[{};1H", row + 1).as_bytes());
        for _ in 0..120 {
            term.advance("✌️".as_bytes());
        }
    }
    let mut snapshot = FrameSnapshot::default();
    assert!(snapshot.capture(&term, 0).is_ok());
    let allocations = allocation_counter::measure(|| {
        snapshot.invalidate();
        assert!(snapshot.capture(&term, 0).is_ok());
    });
    assert_eq!(allocations.count_total, 0);
    assert_eq!(snapshot.copied_count(), 70);
}

/// Sparse rectangular payload ownership across independently reused publications.
mod multicell;

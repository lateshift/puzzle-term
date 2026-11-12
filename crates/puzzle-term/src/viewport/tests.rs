//! Executable viewport contracts with synthetic output and real ring mutations.
#![allow(
    clippy::expect_used,
    reason = "assertions name retained viewport fixtures"
)]
use super::*;
use scrollbar::MarkerKind;
/// Creates enough numbered history to distinguish following from anchoring.
fn fixture() -> (Terminal, Viewport) {
    let mut term = Terminal::new(12, 3, 20, 100_000);
    let mut viewport = Viewport::default();
    viewport.set_row_height(20);
    term.advance(b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix");
    viewport.reconcile(&mut term, true);
    (term, viewport)
}
/// Appends keep followers live, but preserve a scrolled top LineId.
#[test]
fn following_anchor_and_eviction() {
    let (mut term, mut viewport) = fixture();
    assert!(viewport.following());
    viewport.scroll(&mut term, -2.0);
    let top = term
        .screen()
        .ring
        .retained(viewport.top_index())
        .expect("retained row")
        .id;
    term.advance(b"\r\nseven\r\neight");
    viewport.reconcile(&mut term, true);
    assert_eq!(
        term.screen()
            .ring
            .retained(viewport.top_index())
            .expect("retained row")
            .id,
        top
    );
    assert_eq!(viewport.scroll_px(), 80.0);
    assert!(viewport.new_output());
    term.primary.ring.set_limits(1, 100_000, &mut term.tracked);
    viewport.reconcile(&mut term, false);
    assert_eq!(viewport.top_index(), 0);
    assert_eq!(viewport.scroll_px(), 20.0);
    viewport.navigate(&mut term, ScrollTarget::Bottom);
    assert!(!viewport.new_output());
    term.advance(b"\r\nnine");
    viewport.reconcile(&mut term, true);
    assert!(viewport.following());
}
/// Output opt-in follows; ordinary output remains anchored by default.
#[test]
fn output_policy() {
    let (mut term, mut viewport) = fixture();
    viewport.scroll(&mut term, -1.0);
    viewport.on_output = true;
    term.advance(b"X");
    viewport.reconcile(&mut term, true);
    assert!(viewport.following());
    assert!(!viewport.new_output());
}
/// Reflow follows the logical text; row resize, clears and RIS remain defined.
#[test]
fn transformations() {
    let (mut term, mut viewport) = fixture();
    viewport.navigate(&mut term, ScrollTarget::Top);
    term.resize(6, 3);
    viewport.reconcile(&mut term, false);
    assert_eq!(viewport.top_index(), 0);
    let row = term.screen().ring.retained(0).expect("retained row");
    assert_eq!(row.cells[0].ch, u32::from('o'));
    term.resize(6, 2);
    viewport.reconcile(&mut term, false);
    assert_eq!(viewport.top_index(), 0);
    term.advance(b"\x1b[?1049h\x1b[?1049l");
    viewport.reconcile(&mut term, true);
    assert!(viewport.following());
    viewport.navigate(&mut term, ScrollTarget::Top);
    term.advance(b"\x1b[3J");
    viewport.reconcile(&mut term, true);
    assert!(viewport.following());
    term.advance(b"\r\nmore\r\nmore\r\nmore");
    viewport.reconcile(&mut term, true);
    viewport.navigate(&mut term, ScrollTarget::Top);
    term.clear_preserving_line();
    viewport.reconcile(&mut term, false);
    assert!(viewport.following());
    term.advance(b"\r\nmore\r\nmore\r\nmore");
    viewport.reconcile(&mut term, true);
    viewport.navigate(&mut term, ScrollTarget::Top);
    term.advance(b"\x1bc");
    viewport.reconcile(&mut term, true);
    assert!(viewport.following());
}
/// The future overlay receives all source types and exact track fractions.
#[test]
fn scrollbar_geometry_markers_and_operations() {
    let mut term = Terminal::new(20, 3, 20, 100_000);
    term.advance(
        b"\x1b]133;A\x07prompt\x1b]133;B\x07cmd\x1b]133;C\x07\r\nout\r\n\x1b]133;D;2\x07\r\nend",
    );
    let mut viewport = Viewport::default();
    viewport.set_row_height(10);
    viewport.reconcile(&mut term, true);
    let ids: Vec<_> = term.screen().ring.iter().map(|row| row.id).collect();
    viewport.scrollbar.set_track([100.0, 20.0, 8.0, 80.0]);
    viewport.scrollbar.update(
        &term,
        &[(ids[1], ids[1])],
        Some((ids[1], ids[2])),
        &[ids[2]],
    );
    assert_eq!(viewport.scrollbar.markers().len(), 4);
    assert!(
        viewport
            .scrollbar
            .markers()
            .iter()
            .any(|m| m.kind == MarkerKind::Prompt(Some(2)))
    );
    assert_eq!(viewport.scrollbar.thumb(), [40.0, 60.0]);
    let revision = viewport.scrollbar.revision();
    viewport.drag_thumb_to(&mut term, 0.0);
    assert_eq!(viewport.scrollbar.thumb(), [20.0, 60.0]);
    viewport.page_towards(&mut term, 100.0);
    assert!(viewport.following());
    // Geometry reads and movement do not rebuild source markers on each frame.
    assert_eq!(viewport.scrollbar.revision(), revision);
    viewport.navigate(&mut term, ScrollTarget::Line(ids[1]));
    assert_eq!(viewport.top_index(), 0);
    viewport.navigate(&mut term, ScrollTarget::Match(1));
    assert_eq!(viewport.top_index(), 0); // Prompt boundaries must retain their status after width reflow changes IDs.
    term.resize(6, 3);
    viewport.reconcile(&mut term, false);
    viewport.scrollbar.update(&term, &[], None, &[]);
    assert!(
        viewport
            .scrollbar
            .markers()
            .iter()
            .any(|marker| marker.kind == MarkerKind::Prompt(Some(2)))
    );
}

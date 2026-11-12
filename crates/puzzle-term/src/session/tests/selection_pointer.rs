//! M12 text-view gestures use gaps while all other selection/reporting stays cell based.
use super::*;
use crate::{
    pointer::PointerInput,
    vt::input::{Modifiers, MouseEvent, MouseKind},
};

/// Send physical pointer positions through the shared production command path.
fn point(session: &mut TestHost, kind: MouseKind, x: f64, row: usize, modifiers: u8, clicks: u8) {
    let y = row as f64 * 16.0 + 8.0;
    session.submit(
        Command::Pointer(PointerInput {
            event: MouseEvent {
                kind,
                button: 0,
                column: (x.max(0.0) / 8.0) as u32,
                row: row as u32,
                pixel_x: x.max(0.0) as u32,
                pixel_y: y as u32,
                modifiers: Modifiers(modifiers),
                button_held: kind != MouseKind::Release,
                click_count: clicks,
            },
            position: [x, y],
            cell: [8, 16],
        }),
        Origin::User,
    );
    session.pump(Instant::now());
}

/// Default geometry is explicit; no renderer, clocks or desktop service is needed.
fn text(bytes: &[u8], width: usize) -> TestHost {
    let (mut session, _, _) = fixture();
    session.terminal.resize(width, 4);
    session.viewport.set_row_height(16);
    session.feed(bytes, Instant::now());
    session
}

/// Clicking past text starts at its true end; full middle rows still paint to the edge.
#[test]
fn trailing_blank_endpoints_snap_and_intermediate_rows_stay_full() {
    let mut session = text(b"abc\r\ndef\r\nghi", 12);
    point(&mut session, MouseKind::Press, 52.0, 0, 0, 1);
    point(&mut session, MouseKind::Move, 60.0, 2, 0, 1);
    assert_eq!(session.selection_text(), "\ndef\nghi");
    let range = session.selection_range().expect("cross-row range");
    let ring = &session.terminal.screen().ring;
    let middle = ring.retained(ring.scrollback_len() + 1).expect("middle").id;
    assert_eq!(range.start.column, 3);
    assert_eq!(range.end.column, 2);
    assert_eq!(range.columns(middle, 12), Some((0, 12)));
    // Reverse gestures resolve the same document boundaries.
    point(&mut session, MouseKind::Release, 60.0, 2, 0, 1);
    point(&mut session, MouseKind::Press, 60.0, 2, 0, 1);
    point(&mut session, MouseKind::Move, 52.0, 0, 0, 1);
    assert_eq!(session.selection_text(), "\ndef\nghi");
}

/// The midpoint governs both anchor and moving boundary, in either direction.
#[test]
fn midpoint_thresholds_are_mirrored_and_empty_drags_clear() {
    let mut session = text(b"abcdef", 12);
    point(&mut session, MouseKind::Press, 1.0, 0, 0, 1);
    point(&mut session, MouseKind::Move, 3.9, 0, 0, 1);
    assert!(session.selection_range().is_none());
    point(&mut session, MouseKind::Move, 4.1, 0, 0, 1);
    assert_eq!(session.selection_text(), "a");
    point(&mut session, MouseKind::Release, 4.1, 0, 0, 1);
    point(&mut session, MouseKind::Press, 20.1, 0, 0, 1);
    point(&mut session, MouseKind::Move, 12.1, 0, 0, 1);
    assert_eq!(session.selection_text(), "c");
    point(&mut session, MouseKind::Move, 11.9, 0, 0, 1);
    assert_eq!(session.selection_text(), "bc");
    point(&mut session, MouseKind::Move, 20.1, 0, 0, 1);
    assert!(session.selection_range().is_none());
}

/// A wrapped suffix is real text; styled blanks are never default padding.
#[test]
fn soft_wrap_and_styled_blanks_do_not_snap() {
    let mut session = text(b"ab    c", 6);
    point(&mut session, MouseKind::Press, 25.0, 0, 0, 1);
    point(&mut session, MouseKind::Move, 5.0, 1, 0, 1);
    assert_eq!(session.selection_range().expect("wrapped").start.column, 3);
    assert_eq!(session.selection_text(), "   c");
    let mut session = text(b"abc\x1b[44m\x1b[K\x1b[0m\r\ndef", 12);
    point(&mut session, MouseKind::Press, 49.0, 0, 0, 1);
    point(&mut session, MouseKind::Move, 5.0, 1, 0, 1);
    assert_eq!(session.selection_range().expect("styled").start.column, 6);
}

/// Wide clusters and scaled rectangles use their whole footprint's midpoint.
#[test]
fn wide_and_multicell_text_are_indivisible() {
    let mut session = text("a界e\u{301}z".as_bytes(), 12);
    point(&mut session, MouseKind::Press, 8.1, 0, 0, 1);
    point(&mut session, MouseKind::Move, 15.9, 0, 0, 1);
    assert!(session.selection_range().is_none());
    point(&mut session, MouseKind::Move, 16.1, 0, 0, 1);
    assert_eq!(session.selection_text(), "界");
    point(&mut session, MouseKind::Release, 16.1, 0, 0, 1);
    point(&mut session, MouseKind::Press, 24.1, 0, 0, 1);
    point(&mut session, MouseKind::Move, 28.1, 0, 0, 1);
    assert_eq!(session.selection_text(), "e\u{301}");
    let mut session = text(b"\x1b]66;s=2;A\x1b\\", 12);
    point(&mut session, MouseKind::Press, 0.1, 1, 0, 1);
    point(&mut session, MouseKind::Move, 8.1, 1, 0, 1);
    assert_eq!(session.selection_text(), "A");
    let range = session.selection_range().expect("rectangle");
    assert_ne!(range.start.line, range.end.line);
    assert_eq!((range.start.column, range.end.column), (0, 1));
}

/// The live setting preserves blank-cell endpoints; copy still follows trimTrailing.
#[test]
fn line_end_snap_can_be_disabled_live() {
    let mut session = text(b"abc\r\ndef", 12);
    assert!(crate::settings::Settings::default().selection_line_end_snap);
    session.apply_settings(
        &crate::settings::Settings {
            selection_line_end_snap: false,
            selection_trim_trailing: false,
            ..Default::default()
        },
        Instant::now(),
    );
    point(&mut session, MouseKind::Press, 49.0, 0, 0, 1);
    point(&mut session, MouseKind::Move, 5.0, 1, 0, 1);
    assert_eq!(session.selection_range().expect("no snap").start.column, 6);
    assert_eq!(session.selection_text(), "      \nd");
}

/// Word, logical-line and rectangular gestures keep inclusive cell semantics.
#[test]
fn expanded_and_rectangular_gestures_remain_cell_based() {
    let mut session = text(b"ab cd\r\nef gh", 12);
    point(&mut session, MouseKind::Press, 12.1, 0, 0, 2);
    assert_eq!(session.selection_text(), "ab");
    point(&mut session, MouseKind::Release, 12.1, 0, 0, 2);
    point(&mut session, MouseKind::Press, 12.1, 0, 0, 3);
    assert_eq!(session.selection_text(), "ab cd");
    point(&mut session, MouseKind::Release, 12.1, 0, 0, 3);
    point(&mut session, MouseKind::Press, 1.0, 0, Modifiers::ALT, 1);
    point(&mut session, MouseKind::Move, 12.1, 1, Modifiers::ALT, 1);
    assert_eq!(session.selection_text(), "ab\nef\n");
}

/// Application mouse reports always floor physical cells, independent of text edges.
#[test]
fn midpoint_does_not_change_sgr_mouse_reporting() {
    let (mut session, writes, _) = fixture();
    session.feed(b"\x1b[?1000h\x1b[?1006h", Instant::now());
    point(&mut session, MouseKind::Press, 7.0, 0, 0, 1);
    assert_eq!(writes.lock().expect("writes").concat(), b"\x1b[<0;1;1M");
}

/// Zero-column endpoints include only the preceding hard break, with no extra cell.
#[test]
fn row_edge_gaps_and_empty_lines_copy_only_the_selected_breaks() {
    for (bytes, x) in [(b"abcdef\r\ng".as_slice(), 48.0), (b"\r\ng", 30.0)] {
        let mut session = text(bytes, 6);
        point(&mut session, MouseKind::Press, x, 0, 0, 1);
        point(&mut session, MouseKind::Move, 0.0, 1, 0, 1);
        assert_eq!(session.selection_text(), "\n");
    }
}

/// A gap at the final cell survives output and primary reflow like ordinary anchors.
#[test]
fn line_end_boundary_survives_reflow() {
    let mut session = text(b"abcdef\r\nghij", 6);
    let context = super::selection::context();
    session.set_text_context(&context);
    // Use the real font's device geometry through the same owner hit-test path.
    let width = f64::from(context.metrics().width_px);
    let height = f64::from(context.metrics().height_px);
    let mut send = |kind, x: f64, y: f64| {
        let mut sample = super::pointer::sample(kind, y);
        sample.position = [x, y];
        session.submit(Command::Pointer(sample), Origin::User);
        session.pump(Instant::now());
    };
    send(MouseKind::Press, width * 6.0, height / 2.0);
    send(MouseKind::Move, width * 2.1, height * 1.5);
    send(MouseKind::Release, width * 2.1, height * 1.5);
    assert_eq!(session.selection_text(), "\ngh");
    session.resize_grid(4, 4, [100, 100]).expect("reflow");
    assert_eq!(session.selection_text(), "\ngh");
}

/// Shift retains the original gap, including a right-to-left drag's direction.
#[test]
fn shift_extension_retains_the_original_boundary_anchor() {
    let mut session = text(b"abcdef", 12);
    point(&mut session, MouseKind::Press, 40.1, 0, 0, 1);
    point(&mut session, MouseKind::Move, 24.1, 0, 0, 1);
    point(&mut session, MouseKind::Release, 24.1, 0, 0, 1);
    assert_eq!(session.selection_text(), "de");
    point(&mut session, MouseKind::Press, 8.1, 0, Modifiers::SHIFT, 1);
    assert_eq!(session.selection_text(), "bcde");
}

/// Opposite physical edges of a soft wrap are the same logical insertion gap.
#[test]
fn equivalent_soft_wrap_edges_do_not_create_an_empty_selection() {
    let mut session = text(b"abcdefg", 6);
    point(&mut session, MouseKind::Press, 48.0, 0, 0, 1);
    point(&mut session, MouseKind::Move, 0.0, 1, 0, 1);
    assert!(session.selection_range().is_none());
    assert_eq!(session.selection_text(), "");
    point(&mut session, MouseKind::Move, 8.0, 1, 0, 1);
    assert_eq!(session.selection_text(), "g");
}

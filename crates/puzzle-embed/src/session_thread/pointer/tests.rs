//! Host mapping preserves padded geometry, signed drags and fractional selection.
use super::*;
use puzzle_term::{
    Origin, SessionConfig,
    overlay::geometry::TextGeometry,
    platform::{PtySize, Services, Transport, Waker},
    vt::input::{Modifiers, MouseEvent, MouseKind},
};

/// Deliberately bogus protocol fields prove the host cannot supply cell geometry.
fn sample(position: [f64; 2], kind: MouseKind) -> SurfacePointer {
    SurfacePointer {
        position,
        event: MouseEvent {
            kind,
            button: 0,
            column: 999,
            row: 999,
            pixel_x: 999,
            pixel_y: 999,
            modifiers: Modifiers(0),
            button_held: true,
            click_count: 1,
        },
    }
}
/// Both scales and zero/nonzero padding map exclusive cell edges identically.
#[test]
fn view_local_points_use_presented_padding_scale_and_grid() {
    for scale in [1.0, 2.0] {
        for padding in [0.0, 4.0, 8.0] {
            let geometry = TextGeometry {
                cell: [9.0, 18.0],
                inset: padding,
                dpr: scale,
                ..Default::default()
            };
            for (point, expected) in [
                ([4.5, 9.0], [0, 0]),
                ([8.99, 17.99], [0, 0]),
                ([9.01, 18.01], [1, 1]),
                ([-0.01, -0.01], [0, 0]),
                ([1000.0, 1000.0], [7, 3]),
            ] {
                let input = map(
                    sample(point.map(|n| n + f64::from(padding)), MouseKind::Press),
                    &geometry,
                    (8, 4),
                );
                assert_eq!([input.event.column, input.event.row], expected);
                for (actual, expected) in input
                    .position
                    .into_iter()
                    .zip(point.map(|n| n * f64::from(scale)))
                {
                    assert!((actual - expected).abs() < 1e-9);
                }
            }
        }
    }
}
/// Selection policy needs no OS callbacks or transport side effects.
struct Sink;
impl Transport for Sink {
    fn write(&mut self, _: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn resize(&mut self, _: PtySize) -> Result<(), String> {
        Ok(())
    }
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}
impl Waker for Sink {
    fn request_frame(&mut self) {}
    fn wake_at(&mut self, _: Instant) {}
}
/// History and a fractional row shift remain core policy, not a host correction.
#[test]
fn view_local_selection_uses_scrolled_rows_without_double_shift() {
    for dpr in [1.0, 2.0] {
        let (mut session, metrics) = selection_session(dpr);
        let now = Instant::now();
        assert_ne!(session.viewport().device_shift(), 0);
        let geometry = TextGeometry {
            cell: [9.0, 18.0],
            inset: 8.0,
            dpr,
            ..Default::default()
        };
        let row = ((9.0 * f64::from(dpr) - f64::from(session.viewport().device_shift()))
            / f64::from(metrics.height_px))
        .floor() as usize;
        let line = session
            .terminal()
            .screen()
            .ring
            .retained(session.viewport().top_index() + row)
            .unwrap_or_else(|| panic!("visible retained row"))
            .id;
        // Start just before the first character's midpoint; the moving edge
        // crosses the third character's midpoint without changing its hit cell.
        for (kind, x) in [(MouseKind::Press, 12.4), (MouseKind::Move, 30.5)] {
            let pointer = map(sample([x, 17.0], kind), &geometry, (8, 4));
            session
                .apply(
                    SessionMessage::Command(Command::Pointer(pointer), Origin::User),
                    now,
                )
                .unwrap_or_else(|error| panic!("{error}"));
        }
        let selection = session
            .selection_range()
            .unwrap_or_else(|| panic!("selection exists"));
        assert_eq!(selection.start.line, line);
        assert_eq!(selection.end.line, line);
        assert_eq!((selection.start.column, selection.end.column), (0, 2));
        assert_eq!(session.selection_text(), "abc");
    }
}

/// Build a history-bearing fractional viewport independently of pointer mapping.
fn selection_session(dpr: f32) -> (Session, puzzle_term::text::metrics::CellMetrics) {
    let mut session = Session::new(
        SessionConfig {
            columns: 8,
            rows: 4,
            ..Default::default()
        },
        Services::new(Box::new(Sink), Box::new(Sink)),
    );
    let now = Instant::now();
    let settings = puzzle_term::settings::Settings {
        scrolling_smooth: false,
        scrolling_snap_to_rows: false,
        ..Default::default()
    };
    session.apply_settings(&settings, now);
    let metrics = puzzle_term::text::metrics::CellMetrics {
        width_px: (9.0 * dpr) as u32,
        height_px: (18.0 * dpr) as u32,
        baseline_px: (14.0 * dpr) as i32,
        device_size: 15.0 * dpr,
        cap_snapped: false,
        underline_px: 1,
    };
    session
        .apply(
            SessionMessage::FontMetrics(puzzle_term::FontMetrics {
                cell: metrics,
                font_size: 15.0,
                dpr,
                atlas_generation: 0,
            }),
            now,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    for _ in 0..12 {
        session.feed(b"abcdefgh\r\n", now);
    }
    let mut event = sample([0.0, 0.0], MouseKind::Wheel).event;
    event.modifiers = Modifiers(Modifiers::SHIFT);
    session
        .apply(
            SessionMessage::Command(
                Command::Wheel {
                    delta: ScrollInput::Pixels(-25.0 * f64::from(dpr)),
                    pointer: event,
                },
                Origin::User,
            ),
            now,
        )
        .unwrap_or_else(|error| panic!("{error}"));
    (session, metrics)
}

/// The reserved UI cell is not application column one; host points stay view-local.
#[test]
fn gutter_pointer_mapping_matches_rendered_cell_edges() {
    use puzzle_term::overlay::geometry::Rect;
    for dpr in [1.0, 2.0] {
        let geometry = TextGeometry {
            cell: [9.0, 18.0],
            inset: 8.0,
            dpr,
            columns: 8,
            grid: Rect {
                origin: [17.0, 8.0],
                size: [72.0, 72.0],
            },
            ..Default::default()
        };
        for (x, column) in [(17.0, 0), (21.5, 0), (25.999, 0), (26.0, 1), (88.99, 7)] {
            let input = map(sample([x, 17.0], MouseKind::Press), &geometry, (8, 4));
            assert_eq!((input.event.column, input.event.row), (column, 0));
            assert_eq!(input.position[0], (x - 17.0) * f64::from(dpr));
        }
    }
}

/// Exact press/moving edges share one nearest-boundary rule in both directions.
#[test]
fn subcell_selection_ranges_match_padded_scaled_and_gutter_geometry() {
    use puzzle_term::overlay::geometry::Rect;
    for dpr in [1.0, 2.0] {
        for padding in [0.0, 4.0, 8.0] {
            for gutter_columns in [0.0, 2.0] {
                let geometry = TextGeometry {
                    cell: [9.0, 18.0],
                    inset: padding,
                    dpr,
                    columns: 20,
                    grid: Rect {
                        origin: [padding + gutter_columns * 9.0, padding],
                        size: [180.0, 72.0],
                    },
                    ..Default::default()
                };
                for (x, gap) in [
                    (1.25, 1),
                    (1.5, 2),
                    (1.75, 2),
                    (2.25, 2),
                    (2.5, 3),
                    (2.75, 3),
                ] {
                    // The variable endpoint is a press and an end, leftward and rightward.
                    for (a, b, first, last) in [
                        (x, 6.0, gap, 5),
                        (6.0, x, gap, 5),
                        (0.0, x, 0, gap - 1),
                        (x, 0.0, 0, gap - 1),
                    ] {
                        assert_subcell_range(&geometry, a, b, (first, last));
                    }
                }
            }
        }
    }
}

/// Feed view-local logical points through the exact mapping used by both native hosts.
fn assert_subcell_range(geometry: &TextGeometry, a: f64, b: f64, expected: (usize, usize)) {
    let (mut session, _) = selection_session(geometry.dpr);
    let now = Instant::now();
    // Reset removes the history fixture and its fractional viewport before this row.
    session.feed(b"\x1bc", now);
    session
        .resize_grid(20, 4, [400, 200])
        .unwrap_or_else(|e| panic!("{e}"));
    session.feed(b"! sooo excited", now);
    for (kind, column) in [
        (MouseKind::Press, a),
        (MouseKind::Move, b),
        (MouseKind::Release, b),
    ] {
        let position = [
            f64::from(geometry.origin()[0]) + column * 9.0,
            f64::from(geometry.origin()[1]) + 9.0,
        ];
        let input = map(sample(position, kind), geometry, (20, 4));
        // Protocol hits still floor; sub-cell precision survives independently.
        assert_eq!(input.event.column, column.floor() as u32);
        assert_eq!(input.position[0], column * 9.0 * f64::from(geometry.dpr));
        session
            .apply(
                SessionMessage::Command(Command::Pointer(input), Origin::User),
                now,
            )
            .unwrap_or_else(|e| panic!("{e}"));
    }
    let range = session
        .selection_range()
        .unwrap_or_else(|| panic!("range at {a}→{b}"));
    assert_eq!(
        (range.start.column, range.end.column),
        expected,
        "{a}→{b}, DPR {}, origin {:?}",
        geometry.dpr,
        geometry.origin()
    );
    assert_eq!(range.start.line, range.end.line);
    if expected.1 == 5 {
        assert_eq!(session.selection_text(), &"! sooo excited"[expected.0..=5]);
    }
}

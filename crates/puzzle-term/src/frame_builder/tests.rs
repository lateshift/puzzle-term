//! Real-font frame construction and cache evidence for §10.4 and P1.
//!
//! The bundled TTF is read only by tests, never embedded in this neutral
//! crate. Repeating an identical visible row must reuse its Slug handle and
//! cached instances; changing one cell must rebuild only that row.
#![allow(
    clippy::expect_used,
    reason = "test fixtures and assertions report setup failures"
)]
use super::context::TextContext;
use super::*;

/// A new snapshot cannot discard bytes for glyph handles cached by a skipped frame.
#[test]
fn unpresented_frame_keeps_pending_atlas_uploads() {
    let mut context = TextContext::new(fonts(), 14.0, 2.0).expect("metrics");
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(26, 2, 0, 0);
    let mut frame = RenderFrame::default();
    terminal.advance(b"abcdefghijklmnopqrstuvwxyz");
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("unavailable surface frame");
    let count = frame.uploads.len();
    assert!(count > 0);
    // Timeout/loss, zero size, hidden pages and a dropped snapshot all omit the
    // GPU consumer. Row-cache reuse must carry those same pending bytes forward.
    terminal.advance(b"\r\n");
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("next frame");
    assert_eq!(
        frame.uploads.len(),
        count,
        "skipped frame consumed unwritten atlas data"
    );
}
use crate::session::cursor_settings::{CursorBlink, CursorColor, CursorSettings, CursorStyle};
use puzzle_frame::ZSlot;
use puzzle_text::{FontData, font_set::FontSet};

/// Physical cell ownership survives shaping, wrapping and cursor recoloring.
mod cell_ownership;
/// Equal-color ink diagnostics retain the submitted palette and interaction policy.
mod contrast;
/// Background results must reach shared atlases even after their cells scroll away.
mod fallback;
/// Backend dispatch skips impossible probes while preserving mask reuse.
mod ink_dispatch;
/// Three independently reused publication slots must survive skipped frames.
mod rotating_slots;
/// Exposed history rows must retain scratch storage across fractional edge changes.
mod scrolling;

/// Loads a platform-owned font fixture without including its bytes in wasm.
fn fonts() -> FontSet {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/JetBrainsMono-Variable.ttf"
    );
    let bytes = std::fs::read(path).expect("bundled font fixture");
    FontSet::new(FontData::new(bytes, 0), None, None).expect("valid bundled font")
}

/// A shrunken surface clips old VT rows until the PTY drag timer expires.
#[test]
fn pending_resize_never_scissors_beyond_backing_pixels() {
    // Reproduces the first live M6.4 run's `frame scissor exceeds target`
    // validation failure when the window starts narrower than 80 columns.
    let clip = padding::clip([300, 90], 800, 480, 16);
    assert_eq!((clip.x, clip.y, clip.width, clip.height), (16, 16, 284, 74));
    let tiny = padding::clip([1, 1], 800, 480, 16);
    assert_eq!((tiny.x, tiny.y, tiny.width, tiny.height), (1, 1, 0, 0));
}

/// Padding and the incomplete final cell use the theme default background.
#[test]
fn frame_clear_matches_default_background() {
    let mut context = TextContext::new(fonts(), 14.0, 2.0).expect("valid cell metrics");
    let mut builder = FrameBuilder::new(&context);
    let terminal = Terminal::new(3, 1, 0, 0);
    let mut frame = RenderFrame::default();
    builder.set_surface_size([123, 77]);
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("frame");
    // An unfilled edge must match default-background cells exactly;
    // the M0 surface clear would otherwise appear as a grey border.
    assert_eq!(frame.clear, [14.0 / 255.0, 18.0 / 255.0, 26.0 / 255.0, 1.0]);
}

/// Changing one line preserves another line's prepared cache and draw data.
#[test]
fn unchanged_row_reuses_its_geometry() {
    let mut context = TextContext::new(fonts(), 14.0, 1.0).expect("valid cell metrics");
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(12, 2, 0, 0);
    let mut frame = RenderFrame::default();
    terminal.advance(b"ABC\r\nxyz");
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("first frame");
    assert_eq!(builder.rebuilt_rows(), 2);
    assert!(!frame.slugs.is_empty());
    assert!(!frame.uploads.is_empty());
    let count = frame.slugs.len();
    // This fixture models a successful GPU consumer before the cached rebuild.
    frame.uploads.clear();
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("cached frame");
    assert_eq!(builder.rebuilt_rows(), 0);
    assert_eq!(frame.slugs.len(), count);
    assert!(frame.uploads.is_empty());
    terminal.advance(b"\rZ");
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("one damaged row");
    assert_eq!(builder.rebuilt_rows(), 1);
}

/// Warm frame assembly, including unchanged row copies, requests no heap.
#[test]
fn warm_frame_assembly_allocates_nothing() {
    let mut context = TextContext::new(fonts(), 14.0, 1.0).expect("valid cell metrics");
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(12, 2, 0, 0);
    let mut frame = RenderFrame::default();
    terminal.advance(b"ABC\r\nxyz");
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("warm glyphs");
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("warm arrays");
    let measured = allocation_counter::measure(|| {
        for _ in 0..8 {
            builder
                .build(&mut context, &terminal, &mut frame)
                .expect("allocation-free cache hit");
        }
    });
    assert_eq!(
        measured.count_total, 0,
        "warm frame should reuse buffers: {measured:?}"
    );
}

/// DECSCUSR geometry and DECTCEM visibility must survive frame caching.
#[test]
fn cursor_styles_follow_terminal_state() {
    let mut context = TextContext::new(fonts(), 14.0, 1.0).expect("valid cell metrics");
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(8, 1, 0, 0);
    let mut frame = RenderFrame::default();
    terminal.advance(b"A\x1b[6 q");
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("bar cursor");
    let bar = frame.rects.last().expect("cursor rectangle");
    assert_eq!(bar.rect[2], 1.0);
    assert_eq!(bar.rect[3], builder.metrics().height_px as f32);
    terminal.advance(b"\x1b[4 q");
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("underline cursor");
    let underline = frame.rects.last().expect("cursor rectangle");
    assert_eq!(underline.rect[3], 1.0);
    terminal.advance(b"\x1b[?25l");
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("hidden cursor");
    assert!(
        !frame
            .batches
            .iter()
            .any(|batch| batch.slot == ZSlot::Cursor)
    );
}

/// SGR 58 colors a shared Slug wave without changing its prepared outline.
#[test]
fn curly_underline_tiles_and_uses_underline_color() {
    let mut context = TextContext::new(fonts(), 14.0, 1.0).expect("valid cell metrics");
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(8, 1, 0, 0);
    let mut frame = RenderFrame::default();
    // xterm's colon SGR form selects curly style 3 and RGB underline ink.
    terminal.advance(b"\x1b[4:3;58:2::255:0:0mAB");
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("curly underline");
    let waves = &builder.rows[0].waves;
    assert_eq!(waves.len(), 2);
    assert_eq!(waves[0].glyph_loc, waves[1].glyph_loc);
    assert_eq!(
        waves[1].t[0] - waves[0].t[0],
        builder.metrics().width_px as f32
    );
    assert_eq!(waves[0].color, colors::rgba([255, 0, 0]));
}

/// Dotted and dashed SGR styles emit snapped bars instead of a solid line.
#[test]
fn patterned_underlines_have_gaps() {
    let mut context = TextContext::new(fonts(), 14.0, 1.0).expect("valid cell metrics");
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(8, 1, 0, 0);
    let mut frame = RenderFrame::default();
    terminal.advance(b"\x1b[4:4mA\x1b[4:5mB");
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("patterned underlines");
    let bars = &builder.rows[0].decorations;
    assert!(bars.len() > 2);
    assert!(
        bars.windows(2)
            .any(|pair| pair[0].rect[0] + pair[0].rect[2] < pair[1].rect[0])
    );
}

/// Inverse cursor color uses the existing colored cell, not theme defaults.
#[test]
fn cursor_inverse_and_literal_color_follow_osc_precedence() {
    let mut context = TextContext::new(fonts(), 14.0, 1.0).expect("valid cell metrics");
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(4, 1, 0, 0);
    let mut frame = RenderFrame::default();
    terminal.advance(b"\x1b[38;2;255;0;0;48;2;0;0;255mA\x1b[1G");
    let settings = CursorSettings {
        color: CursorColor::Inverse,
        ..CursorSettings::default()
    };
    builder.set_cursor_settings(settings);
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("inverse cursor");
    assert_eq!(
        frame.rects.last().expect("cursor rect").color,
        colors::rgba([255, 0, 0])
    );
    assert_eq!(
        frame.slugs.last().expect("cursor ink").color,
        colors::rgba([0, 0, 255])
    );
    // A setting change affects only appearance; the VT row version and
    // cell contents remain identical while the next frame uses new RGB.
    let before = terminal.screen().row(0).expect("row").version;
    let epoch = builder.style_epoch();
    builder.set_cursor_settings(CursorSettings {
        color: CursorColor::Fixed([0, 255, 0]),
        ..settings
    });
    assert!(builder.style_epoch() > epoch);
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("fixed cursor");
    assert_eq!(terminal.screen().row(0).expect("row").version, before);
    assert_eq!(
        frame.rects.last().expect("cursor rect").color,
        colors::rgba([0, 255, 0])
    );
    // OSC 12 is strongest until OSC 112 removes the application override.
    terminal.advance(b"\x1b]12;#010203\x07");
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("OSC cursor");
    assert_eq!(
        frame.rects.last().expect("cursor rect").color,
        colors::rgba([1, 2, 3])
    );
    terminal.advance(b"\x1b]112\x07");
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("restored user cursor");
    assert_eq!(
        frame.rects.last().expect("cursor rect").color,
        colors::rgba([0, 255, 0])
    );
}

/// A live style update changes geometry without a VT edit or font reload.
#[test]
fn cursor_setting_changes_next_frame_without_grid_damage() {
    let mut context = TextContext::new(fonts(), 14.0, 1.0).expect("valid cell metrics");
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(4, 1, 0, 0);
    let mut frame = RenderFrame::default();
    terminal.advance(b"A\x1b[1G\x1b[4 q");
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("app underline");
    assert_eq!(frame.rects.last().expect("cursor").rect[3], 1.0);
    builder.set_cursor_settings(CursorSettings {
        style: CursorStyle::Bar,
        blink: CursorBlink::Off,
        allow_app_style: false,
        ..CursorSettings::default()
    });
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("user bar");
    assert_eq!(frame.rects.last().expect("cursor").rect[2], 1.0);
    assert_eq!(terminal.cursor_style, 4);
}

/// One atlas serves two row caches and invalidates both after a reset.
#[test]
fn shared_context_prepares_once_and_rebuilds_both_generations() {
    let mut context = TextContext::new(fonts(), 14.0, 1.0).expect("valid context");
    // Each session owns row caches, but neither builder receives fonts or
    // constructs an atlas. A second startup therefore performs no font IO.
    let mut first = FrameBuilder::new(&context);
    let mut second = FrameBuilder::new(&context);
    let mut term_a = Terminal::new(4, 1, 0, 0);
    let mut term_b = Terminal::new(4, 1, 0, 0);
    term_a.advance(b"AB");
    term_b.advance(b"AB");
    let mut frame_a = RenderFrame::default();
    let mut frame_b = RenderFrame::default();
    first
        .build(&mut context, &term_a, &mut frame_a)
        .expect("first atlas entry");
    let entries = context.prepared_glyphs();
    assert!(entries >= 2);
    second
        .build(&mut context, &term_b, &mut frame_b)
        .expect("shared atlas hit");
    assert_eq!(context.prepared_glyphs(), entries);
    assert_eq!(first.rebuilt_rows(), 1);
    assert_eq!(second.rebuilt_rows(), 1);
    // A reset drops old handles. The unchanged VT rows must be copied and
    // rebuilt in both builders before their next GPU instance is submitted.
    let old = context.atlas_generation();
    context.reset_atlas();
    assert!(context.atlas_generation() > old);
    first
        .build(&mut context, &term_a, &mut frame_a)
        .expect("first new atlas row");
    assert_eq!(first.rebuilt_rows(), 1);
    second
        .build(&mut context, &term_b, &mut frame_b)
        .expect("second new atlas row");
    assert_eq!(second.rebuilt_rows(), 1);
    assert_eq!(context.prepared_glyphs(), entries);
    assert_eq!(frame_a.slugs.len(), frame_b.slugs.len());
}

/// Cached LineIds keep their ink when whole-row viewport navigation changes y.
#[test]
fn viewport_cached_rows_never_reshape_for_movement() {
    let mut context = TextContext::new(fonts(), 14.0, 1.0).expect("metrics");
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(12, 3, 20, 100_000);
    terminal.advance(b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix");
    terminal.advance(b"\x1b[?25l");
    let mut frame = RenderFrame::default();
    builder
        .build_viewport(&mut context, &terminal, 0, &mut frame)
        .expect("warm top");
    builder
        .build_viewport(&mut context, &terminal, 1, &mut frame)
        .expect("expose row");
    assert_eq!(builder.rebuilt_rows(), 1);
    builder
        .build_viewport(&mut context, &terminal, 0, &mut frame)
        .expect("cached top");
    assert_eq!(builder.rebuilt_rows(), 0);
    builder
        .build_viewport(&mut context, &terminal, 1, &mut frame)
        .expect("cached shift");
    assert_eq!(builder.rebuilt_rows(), 0);
    assert!(!frame.slugs.is_empty());
}

/// Warm selection fills/recoloring reuse frame buffers without reshaping (P1/P4).
#[test]
fn warm_selected_frame_allocates_nothing() {
    use crate::selection::SelectionRange;
    use crate::vt::selection::SelectPoint;
    let mut context = TextContext::new(fonts(), 14.0, 2.0).expect("context");
    let mut builder = FrameBuilder::new(&context);
    let mut term = Terminal::new(12, 3, 0, 0);
    term.advance(b"\x1b[?25lfoo ->bar\r\n\x1b[31mred -> ink");
    let ids: Vec<_> = term
        .buffer()
        .lines(term.buffer().line_ids().expect("ids"))
        .map(|r| r.id())
        .collect();
    let mut theme = crate::theme::ThemeRegistry::default()
        .resolve("puzzle-dark")
        .expect("theme");
    theme.selection_foreground = Some([255, 255, 255]);
    builder.set_theme(&theme);
    builder.set_selection(Some(SelectionRange {
        start: SelectPoint {
            line: ids[0],
            column: 4,
        },
        end: SelectPoint {
            line: ids[1],
            column: 5,
        },
        block: false,
    }));
    let mut frame = RenderFrame::default();
    for _ in 0..3 {
        builder
            .build(&mut context, &term, &mut frame)
            .expect("warm");
    }
    let measured = allocation_counter::measure(|| {
        for _ in 0..8 {
            builder
                .build(&mut context, &term, &mut frame)
                .expect("selected frame");
        }
    });
    assert_eq!(measured.count_total, 0, "{measured:?}");
    assert_eq!(builder.rebuilt_rows(), 0);
    // Clearing the model removes selection paper without touching row versions.
    builder.set_selection(None);
    builder
        .build(&mut context, &term, &mut frame)
        .expect("clear");
    assert_eq!(builder.rebuilt_rows(), 0);
    assert!(
        !frame
            .rects
            .iter()
            .any(|r| r.color == colors::rgba(theme.selection_background))
    );
}

/// Search changes only paper at assembly, preserving warm glyph geometry (P1/P4).
#[test]
fn warm_search_frame_allocates_nothing() {
    use crate::{search::SearchFill, selection::SelectionRange, vt::selection::SelectPoint};
    let mut context = TextContext::new(fonts(), 14.0, 2.0).expect("context");
    let mut builder = FrameBuilder::new(&context);
    let mut term = Terminal::new(12, 3, 0, 0);
    term.advance(b"\x1b[?25lfoo bar bar");
    let id = term.screen().ring.retained(0).expect("row").id;
    let mut fills = [SearchFill {
        range: SelectionRange {
            start: SelectPoint {
                line: id,
                column: 4,
            },
            end: SelectPoint {
                line: id,
                column: 6,
            },
            block: false,
        },
        current: false,
    }];
    let mut frame = RenderFrame::default();
    for _ in 0..3 {
        builder.set_search(&fills);
        builder
            .build(&mut context, &term, &mut frame)
            .expect("warm");
    }
    let ink = frame.slugs.len();
    let measured = allocation_counter::measure(|| {
        for index in 0..8 {
            fills[0].current = index % 2 == 0;
            builder.set_search(&fills);
            builder
                .build(&mut context, &term, &mut frame)
                .expect("search frame");
        }
    });
    assert_eq!(measured.count_total, 0, "{measured:?}");
    assert_eq!(builder.rebuilt_rows(), 0);
    assert_eq!(frame.slugs.len(), ink);
    assert_eq!(frame.rects.len(), 1);
    builder.set_search(&[]);
    builder
        .build(&mut context, &term, &mut frame)
        .expect("clear");
    assert!(frame.rects.is_empty());
    assert_eq!(builder.rebuilt_rows(), 0);
    assert_eq!(frame.slugs.len(), ink);
}

/// Scaled grid blocks preserve glyph, cursor and pointer ownership.
mod multicell;
